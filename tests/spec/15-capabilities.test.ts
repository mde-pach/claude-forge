/**
 * @spec 15 · Engine capabilities
 *
 * A rule says what it needs; the engine decides how. `where: {S: {type: …}}` on an
 * ast-grep metavariable keeps a match only when the static type of the expression
 * bound to `S` is declared in the named package or module, with one of the named
 * declared names (or is a subtype of such a type): never by the names the project
 * chose for its variables, receivers or imports. The types engine is one pinned
 * language server per language (ty for Python, TypeScript 7's `tsc --lsp`), kept
 * running per repository and asked by the hooks through a FIFO. These tests run the
 * generated hooks on projects with the real packages installed.
 */
import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import { cpSync, existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { renderChecks, renderLib } from "../../src/render";
import { ENGINE_PYTHON, TY_VERSION } from "../../src/util";
import { commitAll, fakeClaude, fixture, forge, git, hasBun, hasUv, hookJson, othersRunning, processesNaming, put, ROOT, sh, stateDir, stopEngines, TOOL_VERSIONS } from "../helpers";

const T = 300_000;
const SCRIPTS = ".claude/skills/forge-project/scripts";
/** Per edit, once the engine runs: the hook's type question (client, FIFO and engine), in ms. */
const WARM_BUDGET_MS = 150;
/** The first question of a session, the engine started on demand, in ms. */
const COLD_BUDGET_MS = 5_000;
// a daemon a failed test leaves behind goes away on its own
const ENV = { CLAUDE_CODE_ENTRYPOINT: "cli", FORGE_TYPES_IDLE: "300" };

function hook(repo: string, script: string, fields: Record<string, unknown>, env: Record<string, string> = {}) {
  return sh("sh", [join(SCRIPTS, script)], { cwd: repo, input: hookJson({ cwd: repo, ...fields }), env: { ...ENV, ...env } });
}
const edit = (file: string) => ({ session_id: "types", hook_event_name: "PostToolUse", tool_name: "Edit", tool_input: { file_path: file } });
/** The additionalContext post-edit.sh returns after an edit of <file>, "" when it prints nothing. */
function afterEdit(repo: string, file: string, env: Record<string, string> = {}): string {
  const r = hook(repo, "post-edit.sh", edit(file), env);
  expect(r.status).toBe(0);
  return r.stdout.trim() ? JSON.parse(r.stdout).hookSpecificOutput.additionalContext : "";
}
function daemonPid(repo: string, lang: string): number | undefined {
  const f = join(stateDir(repo), "types", lang, "pid");
  return existsSync(f) ? Number(readFileSync(f, "utf8").trim()) : undefined;
}
function alive(pid: number | undefined): boolean {
  return pid !== undefined && sh("kill", ["-0", String(pid)]).status === 0;
}
/** The engine of <lang> and the processes it started (its language server), by pid. */
function engineProcesses(repo: string, lang: string): string[] {
  const pid = daemonPid(repo, lang);
  if (pid === undefined) return [];
  const tree = (p: string): string[] => [p, ...sh("pgrep", ["-P", p]).stdout.split("\n").filter(Boolean).flatMap(tree)];
  return tree(String(pid));
}
const running = (pids: string[]) => pids.filter((p) => sh("kill", ["-0", p]).status === 0);
/** Waits up to 5 s for <pids> to end; returns those still running. */
function ended(pids: string[]): string[] {
  for (let i = 0; i < 50 && running(pids).length; i++) Bun.sleepSync(100);
  return running(pids);
}

/**
 * A git repository from tests/generator/<sample> (or empty) with <setup> run, committed,
 * and forge applied: a fixture (helpers.ts), built once per run under <key>.
 */
function project(key: string, sample: string | undefined, setup: (dir: string) => void): string {
  return fixture(`types-${key}`, (dir) => {
    if (sample) cpSync(join(ROOT, "tests", "generator", sample), dir, { recursive: true });
    setup(dir);
    git(dir, "init", "-q", "-b", "main");
    commitAll(dir, "init");
    const r = forge(["apply", "--yes"], dir, { ...fakeClaude() });
    if (r.status !== 0) throw new Error(`forge apply failed:\n${r.stdout}\n${r.stderr}`);
    commitAll(dir, "forge");
  });
}

/**
 * The type question an edit asks, on its own: the generated forge_types_filter on
 * ast-grep matches (with their metadata) of <file>, against the repository's running
 * engine. Returns the matches kept and the wall time in ms.
 */
function typeQuestion(repo: string, lang: "python" | "typescript", file: string): { kept: number; ms: number } {
  const tmp = mkdtempSync(join(tmpdir(), "forge-tq-"));
  const scan = sh("sh", ["-c", `. ${SCRIPTS}/lib.sh; astgrep scan -c .forge/ast-grep/sgconfig.yml --json=stream --include-metadata "$1"`, "sh", file], { cwd: repo });
  writeFileSync(join(tmp, "matches"), scan.stdout);
  const script = join(tmp, "q.sh");
  writeFileSync(script, [renderLib(), "IFS='\n'", renderChecks([], [], [lang]), `FORGE_STATE='${stateDir(repo)}'`, 'forge_types_filter "$1"'].join("\n"));
  const t = performance.now();
  const r = sh("sh", [script, join(tmp, "matches")], { cwd: repo, env: ENV });
  const ms = performance.now() - t;
  expect(r.status).toBe(0);
  const kept = readFileSync(join(tmp, "matches"), "utf8").split("\n").filter(Boolean).length;
  rmSync(tmp, { recursive: true, force: true });
  return { kept, ms };
}
const median = (xs: number[]) => [...xs].sort((a, b) => a - b)[Math.floor(xs.length / 2)]!;

/** @rule python/sqlalchemy/no-query-api */
describe.if(hasUv)("Python: the receiver's type, through ty", () => {
  let repo = "";
  beforeAll(() => {
    repo = project("sqlalchemy", "fastapi-app", (dir) => {
      expect(sh("uv", ["venv", "-q", "--python", "3.12", join(dir, ".venv")]).status).toBe(0);
      expect(sh("uv", ["pip", "install", "-q", "--python", join(dir, ".venv/bin/python"), "sqlalchemy==2.0.44"]).status).toBe(0);
    });
  }, T);
  afterAll(() => stopEngines(repo));

  /** Files are written as Claude's Edit would, then post-edit.sh runs on them: that is how the engine learns of a change (§15.3). */
  const DB = "from sqlalchemy.orm import Session as DbSession\n\n__all__ = [\"DbSession\"]\n";
  const REPO = [
    "from sqlalchemy import create_engine",
    "from sqlalchemy.orm import Session as S, sessionmaker",
    "",
    "from app.db import DbSession",
    "",
    "SessionLocal = sessionmaker(create_engine(\"sqlite://\"))",
    "",
    "",
    "class AppSession(S):",
    "    pass",
    "",
    "",
    "def aliased(db: S):",
    "    return db.query(int).first()",
    "",
    "",
    "def reexported(x: DbSession):",
    "    return x.query(int).first()",
    "",
    "",
    "def made():",
    "    with SessionLocal() as s:",
    "        return s.query(int).count()",
    "",
    "",
    "def subclassed(a: AppSession):",
    "    return a.query(int).all()",
    "",
  ].join("\n");

  test(
    "a receiver of SQLAlchemy's Session is matched whatever it is called: an import alias, a project re-export, sessionmaker()'s result, a subclass",
    () => {
      put(repo, "app/db.py", DB);
      afterEdit(repo, "app/db.py");
      put(repo, "app/repo.py", REPO);
      const t = performance.now();
      const ctx = afterEdit(repo, "app/repo.py");
      expect(performance.now() - t).toBeLessThan(COLD_BUDGET_MS + 2_000);
      for (const line of [14, 18, 23, 27]) expect(ctx).toContain(`forge:python/sqlalchemy/no-query-api app/repo.py:${line}:`);
    },
    T,
  );

  test(
    "a project class named Session, another class's query() and an Any receiver are not matched",
    () => {
      put(
        repo,
        "app/search.py",
        [
          "from typing import Any",
          "",
          "",
          "class Session:",
          "    def query(self, q: str) -> list[str]:",
          "        return [q]",
          "",
          "",
          "class Index:",
          "    def query(self, q: str) -> list[str]:",
          "        return [q]",
          "",
          "",
          "def search(session: Session, index: Index, anything: Any):",
          '    return session.query("a") + index.query("b") + anything.query("c")',
          "",
        ].join("\n"),
      );
      expect(afterEdit(repo, "app/search.py")).toBe("");
    },
    T,
  );

  test(
    "an edit to another file changes the answer: once the re-exported name is the project's own class, its receiver is no longer matched",
    () => {
      put(repo, "app/db.py", DB);
      afterEdit(repo, "app/db.py");
      put(repo, "app/repo.py", REPO);
      expect(afterEdit(repo, "app/repo.py")).toContain("app/repo.py:18:");
      put(repo, "app/db.py", "class DbSession:\n    def query(self, q: object) -> object:\n        return q\n");
      afterEdit(repo, "app/db.py");
      const ctx = afterEdit(repo, "app/repo.py");
      expect(ctx).not.toContain("app/repo.py:18:");
      expect(ctx).toContain("app/repo.py:14:");
    },
    T,
  );

  test.skipIf(othersRunning(import.meta.path))(
    `once the engine runs, an edit's type question (client, FIFO and engine) takes under ${WARM_BUDGET_MS} ms`,
    () => {
      put(repo, "app/db.py", DB);
      afterEdit(repo, "app/db.py");
      put(repo, "app/repo.py", REPO);
      afterEdit(repo, "app/repo.py");
      const times: number[] = [];
      for (let i = 0; i < 9; i++) {
        put(repo, "app/repo.py", `${REPO}\n# edit ${i}\n`);
        const q = typeQuestion(repo, "python", "app/repo.py");
        expect(q.kept).toBe(4);
        times.push(q.ms);
      }
      expect(median(times)).toBeLessThan(WARM_BUDGET_MS);
    },
    T,
  );

  test(
    "the engine SessionStart starts ends with the last session: after SessionEnd neither the engine nor its language server runs",
    () => {
      stopEngines(repo);
      const start = (id: string) => hook(repo, "session-start.sh", { session_id: id, hook_event_name: "SessionStart", source: "startup" });
      const end = (id: string) => hook(repo, "session-end.sh", { session_id: id, hook_event_name: "SessionEnd", reason: "exit" });
      start("life-1");
      start("life-2");
      const sessions = join(stateDir(repo), "types", "python", "sessions");
      for (let i = 0; i < 100 && (engineProcesses(repo, "python").length < 2 || (existsSync(sessions) ? readdirSync(sessions).length : 0) < 2); i++) Bun.sleepSync(100);
      const pids = engineProcesses(repo, "python");
      expect(pids.length).toBeGreaterThanOrEqual(2); // the adapter and ty
      end("life-1");
      Bun.sleepSync(500);
      expect(running(pids)).toEqual(pids); // another session still uses it
      end("life-2");
      expect(ended(pids)).toEqual([]);
    },
    T,
  );

  test(
    "check.sh keeps the same matches, and stops the engine it started when no session uses it",
    () => {
      stopEngines(repo);
      put(repo, "app/db.py", DB);
      afterEdit(repo, "app/db.py");
      put(repo, "app/repo.py", REPO);
      put(repo, "app/search.py", "class Session:\n    def query(self) -> int:\n        return 1\n\n\ndef f(s: Session):\n    return s.query()\n");
      const r = sh("sh", [".forge/check.sh", "--changed"], { cwd: repo, env: ENV });
      expect(r.stdout).toContain("forge:python/sqlalchemy/no-query-api app/repo.py:14:");
      expect(r.stdout).not.toContain("app/search.py");
      expect(alive(daemonPid(repo, "python"))).toBe(false);
      expect(processesNaming(repo)).toEqual([]); // check.sh waited for it to end
    },
    T,
  );

  test(
    "an engine that cannot start is reported with the rules it leaves unchecked, never taken as a pass",
    () => {
      stopEngines(repo);
      const bin = mkdtempSync(join(tmpdir(), "forge-uvx-"));
      const real = sh("sh", ["-c", "command -v uvx"]).stdout.trim();
      writeFileSync(join(bin, "uvx"), `#!/bin/sh\ncase "$*" in *ty==*) echo "no network" >&2; exit 2 ;; esac\nexec "${real}" "$@"\n`);
      sh("chmod", ["+x", join(bin, "uvx")]);
      put(repo, "app/repo.py", `${REPO}\n# again\n`);
      const ctx = afterEdit(repo, "app/repo.py", { PATH: `${bin}:${process.env.PATH}` });
      expect(ctx).toContain("forge:forge/types python/sqlalchemy/no-query-api was not checked: the python engine (ty ");
      expect(ctx).toContain("did not start: exited 2");
    },
    T,
  );

  test(
    "two daemons on one state directory (a start whose starter was killed, racing with the next): the one its pid file does not name exits while lines keep coming, and the other once the state directory and the repository are gone",
    () => {
      const root = mkdtempSync(join(tmpdir(), "forge-twin-root-"));
      writeFileSync(join(root, "a.py"), "x = 1\n");
      const state = mkdtempSync(join(tmpdir(), "forge-twin-state-"));
      // as forge_types_up starts one, twice on the same FIFOs
      const start = sh(
        "sh",
        [
          "-c",
          'mkfifo "$2/in" "$2/started" && exec 7<>"$2/started" || exit 1\n' +
            'for i in 1 2; do uvx -q --python "$3" --from "ty==$4" python "$5" "$1" "$2" < /dev/null >> "$2/log" 2>&1 & done\n' +
            'read -r a <&7; read -r b <&7; echo "$a"; echo "$b"',
          "sh",
          root,
          state,
          ENGINE_PYTHON,
          TY_VERSION,
          join(repo, ".forge/helpers/types_python.py"),
        ],
        { cwd: root, env: ENV },
      );
      expect(start.stdout.split("\n").filter((l) => l.startsWith("ready ty "))).toHaveLength(2);
      const twins = processesNaming(state);
      expect(twins).toHaveLength(2);
      const tree = (p: string): string[] => [p, ...sh("pgrep", ["-P", p]).stdout.split("\n").filter(Boolean).flatMap(tree)];
      const all = twins.flatMap(tree);
      try {
        const owner = readFileSync(join(state, "pid"), "utf8").trim();
        const other = twins.filter((p) => p !== owner);
        expect(other).toHaveLength(1);
        // lines keep coming, each read by one of the two
        for (let i = 0; i < 150 && running(other).length; i++) {
          sh("sh", ["-c", 'printf "changed\\ta.py\\n" 1<>"$1/in"', "sh", state]);
          Bun.sleepSync(100);
        }
        expect(running(other)).toEqual([]);
        expect(running([owner])).toEqual([owner]);
        rmSync(state, { recursive: true, force: true });
        rmSync(root, { recursive: true, force: true });
        for (let i = 0; i < 150 && running(all).length; i++) Bun.sleepSync(100);
        expect(running(all)).toEqual([]);
      } finally {
        for (const p of running(all)) sh("kill", ["-9", p]);
      }
    },
    T,
  );
});

/**
 * The project's own TypeScript answers (§15.2): TypeScript 7 through its language server
 * (tsc --lsp), TypeScript 5 and 6 through their tsserver. The same tests run on one
 * version of each.
 */
/** @rule typescript/zod/v4-string-formats */
describe.each(TOOL_VERSIONS.typescriptEngines.map((v) => [v]))("TypeScript %s: the receiver's type, through the project's TypeScript", (version) => {
  let repo = "";
  beforeAll(() => {
    repo = project(`zod-typescript-${version}`, undefined, (dir) => {
      mkdirSync(join(dir, "src"));
      writeFileSync(join(dir, "package.json"), JSON.stringify({ name: "zod-app", private: true, type: "module", dependencies: { zod: "4.1.12" }, devDependencies: { typescript: version } }));
      writeFileSync(join(dir, "tsconfig.json"), JSON.stringify({ compilerOptions: { strict: true, module: "ESNext", moduleResolution: "bundler", target: "ES2022", noEmit: true } }));
      writeFileSync(join(dir, ".gitignore"), "node_modules/\n");
      writeFileSync(join(dir, "src/index.ts"), "export const x = 1;\n");
      expect(sh("bun", ["install", "--silent"], { cwd: dir }).status).toBe(0);
    });
  }, T);
  afterAll(() => stopEngines(repo));

  const SCHEMAS = [
    'import { z } from "zod";',
    'import * as zod from "zod";',
    "",
    "const base = zod.string().min(1);",
    "export const Email = base.email();",
    "export const Url = z.string().trim().url();",
    "export function id(s: z.ZodString) {",
    "  return s.uuid();",
    "}",
    "",
  ].join("\n");

  test(
    "the engine is the project's TypeScript: tsc --lsp from TypeScript 7, its tsserver before",
    () => {
      put(repo, "src/schemas.ts", SCHEMAS);
      afterEdit(repo, "src/schemas.ts");
      const ready = readFileSync(join(stateDir(repo), "types", "typescript", "ready"), "utf8").trim();
      expect(ready).toBe(Number(version.split(".")[0]) >= 7 ? `typescript-go ${version}` : `tsserver ${version}`);
    },
    T,
  );

  test(
    "a receiver of Zod 4's ZodString is matched whatever it is called: a namespace import, a variable, a parameter, a call chain",
    () => {
      put(repo, "src/schemas.ts", SCHEMAS);
      const ctx = afterEdit(repo, "src/schemas.ts");
      for (const line of [5, 6, 8]) expect(ctx).toContain(`forge:typescript/zod/v4-string-formats src/schemas.ts:${line}:`);
    },
    T,
  );

  test(
    "a project class named ZodString, an object with an email() method and the top-level z.email() are not matched",
    () => {
      put(
        repo,
        "src/mine.ts",
        'import { z } from "zod";\n\nclass ZodString {\n  email(): string {\n    return "";\n  }\n}\n\nexport const a = new ZodString().email();\nexport const user = { email: (): string => "a@b" };\nexport const b = user.email();\nexport const c = z.email();\n',
      );
      expect(afterEdit(repo, "src/mine.ts")).toBe("");
    },
    T,
  );

  test.skipIf(othersRunning(import.meta.path))(
    `once the engine runs, an edit's type question (client, FIFO and engine) takes under ${WARM_BUDGET_MS} ms`,
    () => {
      put(repo, "src/schemas.ts", SCHEMAS);
      afterEdit(repo, "src/schemas.ts");
      const times: number[] = [];
      for (let i = 0; i < 9; i++) {
        put(repo, "src/schemas.ts", `${SCHEMAS}// edit ${i}\n`);
        const q = typeQuestion(repo, "typescript", "src/schemas.ts");
        expect(q.kept).toBe(3);
        times.push(q.ms);
      }
      expect(median(times)).toBeLessThan(WARM_BUDGET_MS);
    },
    T,
  );
});

/** @rule typescript/zod/v4-string-formats */
test(
  "a project without its own TypeScript package gets its typed matches reported unchecked, never passed",
  () => {
    const repo = project("zod-no-typescript", undefined, (dir) => {
      mkdirSync(join(dir, "src"));
      writeFileSync(join(dir, "package.json"), JSON.stringify({ name: "no-ts", private: true, type: "module", dependencies: { zod: "4.1.12" } }));
      writeFileSync(join(dir, "tsconfig.json"), JSON.stringify({ compilerOptions: { strict: true, noEmit: true } }));
      writeFileSync(join(dir, ".gitignore"), "node_modules/\n");
      writeFileSync(join(dir, "src/index.ts"), "export const x = 1;\n");
      expect(sh("bun", ["install", "--silent"], { cwd: dir }).status).toBe(0);
    });
    try {
      put(repo, "src/schemas.ts", 'import { z } from "zod";\n\nexport const Email = z.string().email();\n');
      const ctx = afterEdit(repo, "src/schemas.ts");
      expect(ctx).toContain("forge:forge/types typescript/zod/v4-string-formats was not checked: the typescript engine");
      expect(ctx).toContain("the project has no typescript package");
    } finally {
      stopEngines(repo);
    }
  },
  T,
);

/** @rule python/sqlalchemy/no-query-api */
test(
  "a start whose state directory is removed while the engine is starting still ends: the start timeout's line reaches the starter",
  () => {
    const dir = mkdtempSync(join(tmpdir(), "forge-start-gone-"));
    const state = join(dir, "state");
    // a launcher that hangs (a first download that never ends): the engine never says it is ready
    const bin = join(dir, "bin");
    mkdirSync(bin);
    writeFileSync(join(bin, "uvx"), "#!/bin/sh\nexec sleep 5\n");
    sh("chmod", ["+x", join(bin, "uvx")]);
    const script = join(dir, "start.sh");
    writeFileSync(
      script,
      [
        renderLib(),
        "IFS='\n'",
        renderChecks([], [], ["python"]),
        `FORGE_STATE='${state}'`,
        '( forge_types_up python; echo "rc=$?" ) & p=$!',
        // the state directory goes away while the engine starts (a test's cleanup, a repository deleted)
        `n=0; until [ -p '${state}/types/python/started' ] || [ "$n" -ge 100 ]; do sleep 0.1; n=$((n + 1)); done; rm -rf '${state}'`,
        "( sleep 20; kill $p ) 2>/dev/null & w=$!",
        'wait "$p"; kill "$w" 2>/dev/null',
      ].join("\n"),
    );
    const t = performance.now();
    const r = sh("sh", [script], { cwd: dir, env: { FORGE_TYPES_START_TIMEOUT: "2", PATH: `${bin}:${process.env.PATH}` } });
    expect(r.stdout).toContain("did not start: failed: not started within 2s");
    expect(r.stdout).toContain("rc=1");
    expect(performance.now() - t).toBeLessThan(20_000);
  },
  T,
);
