/**
 * @spec 9 · Shared script conventions
 *
 * Hook scripts share `lib.sh` and are POSIX `sh` only: `[[:space:]]`, never
 * `\s`, `\b` or `grep -P`; `printf '%s'` for data; no `sed -i`. The guard reads
 * `guard.rules` (tab-separated: id, decision, POSIX ERE, description, and an
 * optional check) and answers the first `deny`, else the first `ask`; a rule
 * with a check applies only when its guard.sh check flags the parsed command. These tests run on every
 * shell available: `sh`, `dash`, `bash --posix` (as macOS's bash 3.2) and
 * BusyBox.
 */
import { describe, expect, test } from "bun:test";
import { lstatSync, mkdirSync, mkdtempSync, readdirSync, realpathSync, utimesSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { renderEngine, renderGuardRules, renderLib, resolveEngine, template } from "../../src/render";
import { availableShells, guardDecision, guardExampleDir, hookInput } from "../../src/selftest";
import type { Rule } from "../../src/types";
import { parsePythonEnvReport } from "../../src/audit";
import { formatPythonEnv } from "../../src/cli/commands/status";
import { sh } from "../helpers";

const shells = availableShells();
const lib = mkdtempSync(join(tmpdir(), "forge-lib-"));
writeFileSync(join(lib, "lib.sh"), renderLib());

/**
 * Backdates every file and directory under <dir> by a minute: a fact (§9.4) about a path
 * modified in the same clock tick is not kept, and a shell's `test -nt` may compare whole
 * seconds, so fixtures written just before are made older than the facts about them.
 */
function backdate(dir: string): void {
  const t = new Date(Date.now() - 60_000);
  for (const e of readdirSync(dir, { recursive: true }) as string[]) {
    const p = join(dir, e);
    if (!lstatSync(p).isSymbolicLink()) utimesSync(p, t, t);
  }
  utimesSync(dir, t, t);
}

/** Runs `snippet` after sourcing lib.sh, with $FORGE_INPUT set to `input`. */
/**
 * Two virtual environments of the system's python3, made once: "project", with the
 * metadata of the project's three declared dependencies (flask-login, requests, ruff) and a
 * ruff script, and "other", with none. Undefined without a python3 that has venv.
 */
let venvs: { project: string; other: string } | null | undefined;
function pyVenvs(): { project: string; other: string } | undefined {
  if (venvs === undefined) {
    venvs = null;
    const base = realpathSync(mkdtempSync(join(tmpdir(), "forge-venvs-")));
    const made = ["project", "other"].every((n) => sh("python3", ["-m", "venv", "--without-pip", join(base, n)]).status === 0);
    if (made) {
      const site = sh(join(base, "project/bin/python3"), ["-c", "import sysconfig; print(sysconfig.get_path('purelib'))"]).stdout.trim();
      for (const [dir, name] of [
        ["Flask_Login-0.6.3.dist-info", "Flask-Login"],
        ["forge_fixture_dep-1.0.dist-info", "forge-fixture-dep"],
        ["ruff-0.1.5.dist-info", "ruff"],
      ] as const) {
        mkdirSync(join(site, dir), { recursive: true });
        writeFileSync(join(site, dir, "METADATA"), `Metadata-Version: 2.1\nName: ${name}\nVersion: 1\n`);
      }
      writeFileSync(join(base, "project/bin/ruff"), "#!/bin/sh\n", { mode: 0o755 });
      venvs = { project: join(base, "project"), other: join(base, "other") };
    }
  }
  return venvs ?? undefined;
}

function libRun(argv: string[], input: string, snippet: string, cwd = lib): string {
  const r = sh(argv[0]!, [...argv.slice(1), "-c", `. "${join(lib, "lib.sh")}"; FORGE_INPUT=$(cat); ${snippet}`], { input, cwd });
  return r.stdout;
}

describe.each(shells.map((s) => [s.name, s.argv] as const))("lib.sh on %s", (_name, argv) => {
  test("json_get extracts top-level and tool_input strings and unescapes them", () => {
    const input = JSON.stringify({ cwd: "/a b", tool_input: { file_path: '/x/"q"\\y.py', command: "a\tb\nc \u0001" } });
    expect(libRun([...argv], input, "json_get cwd")).toBe("/a b");
    expect(libRun([...argv], input, "json_get file_path")).toBe('/x/"q"\\y.py');
    expect(libRun([...argv], input, "json_get command")).toBe("a\tb\nc \u0001");
  });

  test("json_get raw keeps the JSON escaping (what the guard matches)", () => {
    expect(libRun([...argv], JSON.stringify({ tool_input: { command: 'echo "x"' } }), "json_get command raw")).toBe('echo \\"x\\"');
  });

  test("json_escape makes any text safe to embed in a JSON string: quotes, backslashes, tabs, newlines and every other control character (carriage returns dropped)", () => {
    const controls = Array.from({ length: 31 }, (_, i) => String.fromCharCode(i + 1))
      .filter((c) => c !== "\n" && c !== "\r")
      .join("");
    const text = `say "hi"\\\n\tnext\u001b[0m bell\u0007 back\b ${controls} end\r\nlast`;
    const out = libRun([...argv], text, 'printf "%s" "$FORGE_INPUT" | json_escape');
    expect(JSON.parse(`"${out}"`)).toBe(text.replaceAll("\r", ""));
  });

  test("tool resolves the project's own binary: .venv/bin in a uv repo, bun run in a bun repo, nothing when absent", () => {
    const uvRepo = mkdtempSync(join(tmpdir(), "forge-uv-"));
    writeFileSync(join(uvRepo, "uv.lock"), "");
    mkdirSync(join(uvRepo, ".venv/bin"), { recursive: true });
    writeFileSync(join(uvRepo, ".venv/bin/ruff"), "#!/bin/sh\n", { mode: 0o755 });
    expect(libRun([...argv], "{}", "PATH=/nonexistent; tool ruff; printf '|'; tool pytest", uvRepo)).toBe(".venv/bin/ruff|");
  });

  /**
   * Projects install their Python tools elsewhere than .venv: an activated virtual
   * environment, uv's UV_PROJECT_ENVIRONMENT, Poetry's own environments, or the
   * interpreter itself (pip, Poetry without virtualenvs, a Docker image). The tool is the
   * one of that environment, else the first on PATH.
   */
  test("tool finds a Python tool in the project's environment outside .venv first, then on PATH", () => {
    const base = realpathSync(mkdtempSync(join(tmpdir(), "forge-pyenv-")));
    const exe = (p: string, body = "") => {
      mkdirSync(dirname(join(base, p)), { recursive: true });
      writeFileSync(join(base, p), `#!/bin/sh\n${body}\n`, { mode: 0o755 });
    };
    // an interpreter on PATH whose scripts directory holds ruff; a global ruff and pytest elsewhere on PATH
    exe("interp/python3", `printf '%s\\n%s\\n%s\\n' "$0" "${base}" "${base}/scripts"`);
    exe("scripts/ruff");
    exe("global/ruff");
    exe("global/pytest");
    exe("venv/bin/python");
    exe("venv/bin/pytest");
    const repo = join(base, "repo");
    mkdirSync(repo);
    const path = `unset VIRTUAL_ENV UV_PROJECT_ENVIRONMENT; PATH="${base}/interp:${base}/global:$PATH"`;
    const all = "tool ruff; printf '|'; tool pytest; printf '|'; tool python; printf '|'; tool tsc";
    expect(libRun([...argv], "{}", `${path}; ${all}`, repo)).toBe(`${base}/scripts/ruff|${base}/global/pytest|${base}/interp/python3|`);
    expect(libRun([...argv], "{}", `${path}; VIRTUAL_ENV="${base}/venv"; ${all}`, repo)).toBe(`${base}/global/ruff|${base}/venv/bin/pytest|${base}/venv/bin/python|`);
    // a uv project's environment is .venv or UV_PROJECT_ENVIRONMENT, nothing else; its tools not there are PATH's
    writeFileSync(join(repo, "uv.lock"), "");
    expect(libRun([...argv], "{}", `${path}; ${all}`, repo)).toBe(`${base}/global/ruff|${base}/global/pytest||`);
    expect(libRun([...argv], "{}", `${path}; UV_PROJECT_ENVIRONMENT="${base}/venv"; tool pytest`, repo)).toBe(`${base}/venv/bin/pytest`);
  });

  test("the declared dependencies are read from [project] (one line or several), [tool.poetry.dependencies] and requirements*.txt, as normalised names", () => {
    const repo = realpathSync(mkdtempSync(join(tmpdir(), "forge-pydecl-")));
    writeFileSync(
      join(repo, "pyproject.toml"),
      '[project]\nname = "x"\ndependencies = ["FastAPI>=0.115", \'SQLAlchemy[asyncio]>=2\',\n  "httpx[http2]",\n  "anyio",\n]\n\n[tool.poetry.dependencies]\npython = "^3.10"\nFlask_Login = "^0.6"\n\n[tool.ruff.lint]\nselect = ["E"]\n',
    );
    writeFileSync(join(repo, "requirements.txt"), "# pinned\nzope.interface==6\n-e .\n");
    expect(
      libRun([...argv], "{}", "forge_py_deps_ask | grep -v '^watch '", repo)
        .trim()
        .split("\n"),
    ).toEqual(["fastapi", "sqlalchemy", "httpx", "anyio", "flask-login", "zope-interface"]);
  });

  /**
   * The project's environment is the one its declared dependencies are installed in, not
   * merely the first interpreter on PATH: an image may put an unrelated virtual environment
   * first (evaluation tooling) while the project's packages and tools are in the system
   * interpreter. Real interpreters (virtual environments with installed distributions'
   * metadata), so the probe is the one the hooks run.
   */
  test.skipIf(!pyVenvs())("the environment with the project's declared dependencies wins over an unrelated one earlier on PATH, whichever comes first; with none, the first one, said so", () => {
    const v = pyVenvs()!;
    const repo = realpathSync(mkdtempSync(join(tmpdir(), "forge-pydeps-")));
    writeFileSync(join(repo, "pyproject.toml"), '[tool.poetry]\nname = "app"\n\n[tool.poetry.dependencies]\npython = "^3.10"\nFlask_Login = "^0.6"\nforge_fixture_dep = "*"\n');
    writeFileSync(join(repo, "requirements-dev.txt"), "# tools\nruff==0.1.5\n-r other.txt\n");
    const run = (path: string) =>
      libRun([...argv], "{}", `unset VIRTUAL_ENV UV_PROJECT_ENVIRONMENT; PATH="${path}:/usr/bin:/bin"; forge_py_env_report`, repo)
        .trim()
        .split("\n");
    // the image's shape: the unrelated environment first on PATH, the project's second
    const shape = run(`${v.other}/bin:${v.project}/bin`);
    expect(shape[0]).toMatch(new RegExp(`^env \\S+ ${v.project} 3 3$`));
    expect(shape).toContain(`checked ${v.other}/bin/python3 0 3`);
    expect(shape).toContain(`tool ruff ${v.project}/bin/ruff env`);
    // the reverse order: still the project's
    expect(run(`${v.project}/bin:${v.other}/bin`)[0]).toMatch(new RegExp(`^env \\S+ ${v.project} 3 3$`));
    // neither has them: the first, with 0 of 3, which forge status says
    const none = run(`${v.other}/bin`);
    expect(none[0]).toMatch(new RegExp(`^env \\S+ ${v.other} 0 3$`));
    const said = formatPythonEnv(parsePythonEnvReport(none.join("\n")));
    expect(said).toStartWith(`Python environment: none has the project's 3 declared dependencies installed (checked: ${v.other}/bin/python3 (0/3)`);
    expect(said).toContain(`; using ${v.other} (${v.other}/bin/python3)`);
    expect(formatPythonEnv(parsePythonEnvReport(shape.join("\n")))).toContain(`  ruff: ${v.project}/bin/ruff (the environment's)`);
    // no declared dependencies: the first interpreter on PATH
    const bare = realpathSync(mkdtempSync(join(tmpdir(), "forge-pydeps-")));
    const r = libRun([...argv], "{}", `unset VIRTUAL_ENV UV_PROJECT_ENVIRONMENT; PATH="${v.other}/bin:${v.project}/bin:/usr/bin:/bin"; forge_py_env_report`, bare)
      .trim()
      .split("\n");
    expect(r[0]).toMatch(new RegExp(`^env \\S+ ${v.other} 0 0$`));
  });

  /**
   * A fact (§9.4) is kept under its key, whatever the key holds: one with newlines or tabs
   * (the ty untyped list's: the interpreter's version and site directories) is found again
   * by the next call, which runs nothing, and a recomputed fact replaces its record instead
   * of adding one.
   */
  test("a fact with a multi-line key is reused by the next call, and its index and records do not grow", () => {
    const base = realpathSync(mkdtempSync(join(tmpdir(), "forge-fact-")));
    const repo = join(base, "repo");
    mkdirSync(repo);
    sh("git", ["init", "-q", repo]);
    writeFileSync(join(base, "dep"), "x\n");
    backdate(base);
    const ask = `ask() { echo run >> "${base}/runs"; printf 'watch %s\\nvalue one\\n' "${base}/dep"; }`;
    const key = "version 3.10\nsite /usr/lib/python3.10/site-packages\tand \\n literal";
    const call = `${ask}; FORGE_STATE_HOME="${base}/st"; forge_fact demo "$(printf '${key}')" ask && printf '%s|' "$FORGE_FACT"`;
    const runs = () => (sh("cat", [join(base, "runs")]).stdout.match(/run/g) ?? []).length;
    for (let i = 0; i < 3; i++) expect(libRun([...argv], "{}", call, repo)).toBe("value one|");
    expect(runs()).toBe(1);
    const facts = sh("sh", ["-c", `ls "${base}"/st/*/facts`])
      .stdout.split("\n")
      .filter(Boolean);
    expect(facts.filter((f) => f.startsWith("demo.") && f !== "demo.index")).toHaveLength(1);
    expect(
      sh("sh", ["-c", `cat "${base}"/st/*/facts/demo.index`])
        .stdout.trim()
        .split("\n"),
    ).toHaveLength(1);
    // what it depends on changes: asked again, and its record replaced, not added to
    writeFileSync(join(base, "dep"), "y\n");
    Bun.sleepSync(1100);
    expect(libRun([...argv], "{}", call, repo)).toBe("value one|");
    expect(runs()).toBe(2);
    const after = sh("sh", ["-c", `ls "${base}"/st/*/facts`])
      .stdout.split("\n")
      .filter((f) => f.startsWith("demo.") && f !== "demo.index");
    expect(after).toHaveLength(1);
    expect(
      sh("sh", ["-c", `cat "${base}"/st/*/facts/demo.index`])
        .stdout.trim()
        .split("\n"),
    ).toHaveLength(1);
  });

  /**
   * Poetry decides where a project's environment is (its name hash, virtualenvs.path,
   * in-project, create = false): Forge asks it (`poetry env info -p`) instead of
   * recomputing its convention, and keeps the answer in the state until Poetry's inputs or
   * its virtualenvs directory change.
   */
  test("tool finds a Python tool in the environment Poetry says is the project's, asking Poetry once until its environments change", () => {
    const base = realpathSync(mkdtempSync(join(tmpdir(), "forge-poetry-")));
    const repo = join(base, "repo");
    mkdirSync(repo);
    sh("git", ["init", "-q", repo]);
    writeFileSync(join(repo, "pyproject.toml"), '[tool.poetry]\nname = "app"\nversion = "0.1.0"\n');
    writeFileSync(join(repo, "poetry.lock"), "");
    const envs = join(base, "envs");
    const env = join(envs, "app-x-py3.12");
    for (const e of [env, join(base, "interp")]) mkdirSync(join(e, "bin"), { recursive: true });
    for (const t of ["python", "ruff"]) writeFileSync(join(env, "bin", t), "#!/bin/sh\n", { mode: 0o755 });
    writeFileSync(join(base, "answer"), `${env}\n`);
    // a fake Poetry: its answer is in <base>/answer ("" when it has no environment), each call is logged
    writeFileSync(
      join(base, "bin-poetry"),
      `#!/bin/sh\necho "$*" >> "${base}/calls"\ncase "$*" in\n  *"config virtualenvs.path"*) echo "${envs}" ;;\n  *"env info -p"*) a=$(cat "${base}/answer"); [ -n "$a" ] || exit 1; echo "$a" ;;\nesac\n`,
      { mode: 0o755 },
    );
    mkdirSync(join(base, "pbin"));
    sh("ln", ["-s", join(base, "bin-poetry"), join(base, "pbin", "poetry")]);
    writeFileSync(join(base, "interp", "bin", "python3"), `#!/bin/sh\nprintf '%s\\n%s\\n%s\\n' "$0" "${base}/interp" "${base}/interp/bin"\n`, { mode: 0o755 });
    writeFileSync(join(base, "interp", "bin", "ruff"), "#!/bin/sh\n", { mode: 0o755 });
    const path = `unset VIRTUAL_ENV UV_PROJECT_ENVIRONMENT; PATH="${base}/pbin:${base}/interp/bin:$PATH"`;
    const calls = () => (sh("cat", [join(base, "calls")]).stdout.match(/env info/g) ?? []).length;
    backdate(base);
    expect(libRun([...argv], "{}", `${path}; tool ruff; printf '|'; tool python`, repo)).toBe(`${env}/bin/ruff|${env}/bin/python`);
    expect(calls()).toBe(1);
    // another shell: Poetry's answer is reused
    expect(libRun([...argv], "{}", `${path}; tool ruff`, repo)).toBe(`${env}/bin/ruff`);
    expect(calls()).toBe(1);
    // Poetry makes another environment (poetry env use 3.13): its virtualenvs directory changed, so it is asked again
    const env2 = join(envs, "app-x-py3.13");
    mkdirSync(join(env2, "bin"), { recursive: true });
    for (const t of ["python", "ruff"]) writeFileSync(join(env2, "bin", t), "#!/bin/sh\n", { mode: 0o755 });
    writeFileSync(join(base, "answer"), `${env2}\n`);
    expect(libRun([...argv], "{}", `${path}; tool ruff`, repo)).toBe(`${env2}/bin/ruff`);
    expect(calls()).toBe(2);
    // virtualenvs.create = false: Poetry has no environment, the project is in the interpreter on PATH
    writeFileSync(join(base, "answer"), "");
    writeFileSync(join(repo, "poetry.toml"), "[virtualenvs]\ncreate = false\n");
    expect(libRun([...argv], "{}", `${path}; tool ruff`, repo)).toBe(`${base}/interp/bin/ruff`);
    // Poetry not on PATH: the interpreter on PATH
    expect(libRun([...argv], "{}", `unset VIRTUAL_ENV UV_PROJECT_ENVIRONMENT; PATH="${base}/interp/bin:$PATH"; tool ruff`, repo)).toBe(`${base}/interp/bin/ruff`);
  });

  /**
   * ty finds .venv, $VIRTUAL_ENV and the environment its config names on its own; any other
   * environment of the project (§9.1) is given to it, since under uvx the python it would find
   * on PATH is uvx's (§15.2).
   */
  test("ty gets --python <sys.prefix> exactly when it would not find the project's environment: .venv, VIRTUAL_ENV, its config, uv, Poetry in-project and external, the interpreter on PATH", () => {
    const base = realpathSync(mkdtempSync(join(tmpdir(), "forge-tyenv-")));
    const script = join(base, "ty-args.sh");
    writeFileSync(script, [renderLib(), "IFS='\n'", renderEngine(["python"], resolveEngine(["forge_ty_args_print"]), { untypedPackages: "source" }), "forge_ty_args_print"].join("\n"));
    const exe = (p: string, body = "") => {
      mkdirSync(dirname(join(base, p)), { recursive: true });
      writeFileSync(join(base, p), `#!/bin/sh\n${body}\n`, { mode: 0o755 });
    };
    exe("interp/bin/python3", `printf '%s\\n%s\\n%s\\n' "$0" "${base}/interp" "${base}/interp/bin"`);
    exe("venv/bin/python");
    exe("poetry-envs/app-x-py3.12/bin/python");
    exe("pbin/poetry", `case "$*" in *"env info -p"*) echo "${base}/poetry-envs/app-x-py3.12" ;; *"config virtualenvs.path"*) echo "${base}/poetry-envs" ;; esac`);
    const repo = (name: string, files: Record<string, string> = {}) => {
      const dir = join(base, name);
      mkdirSync(dir, { recursive: true });
      for (const [f, text] of Object.entries(files)) {
        mkdirSync(dirname(join(dir, f)), { recursive: true });
        writeFileSync(join(dir, f), text, { mode: f.includes("/bin/") ? 0o755 : 0o644 });
      }
      return dir;
    };
    const args = (dir: string, env = "") =>
      sh(argv[0]!, [...argv.slice(1), "-c", `unset VIRTUAL_ENV UV_PROJECT_ENVIRONMENT; PATH="${base}/pbin:${base}/interp/bin:$PATH"; ${env} . "${script}"`], { cwd: dir }).stdout.trim();
    // ty finds these itself
    expect(args(repo("dotvenv", { ".venv/bin/python": "#!/bin/sh\n" }))).toBe("");
    expect(args(repo("activated"), `VIRTUAL_ENV="${base}/venv";`)).toBe("");
    expect(args(repo("configured", { "ty.toml": `[environment]\npython = "${base}/venv"\n` }))).toBe("");
    // Poetry with virtualenvs.in-project: its .venv
    expect(args(repo("poetry-in-project", { "poetry.lock": "", "pyproject.toml": "", ".venv/bin/python": "#!/bin/sh\n" }))).toBe("");
    // these it would not
    expect(args(repo("uv", { "uv.lock": "" }), `UV_PROJECT_ENVIRONMENT="${base}/venv";`)).toBe(`--python\n${base}/venv`);
    expect(args(repo("poetry", { "poetry.lock": "", "pyproject.toml": "" }))).toBe(`--python\n${base}/poetry-envs/app-x-py3.12`);
    expect(args(repo("system"))).toBe(`--python\n${base}/interp`);
  });
});

describe("guard.sh", () => {
  const rule = (id: string, description: string) => ({ id, description, pack: "t", file: `/p/${id}.yaml`, origin: "pack" }) as unknown as Rule;
  const dir = mkdtempSync(join(tmpdir(), "forge-guard-"));
  writeFileSync(join(dir, "guard.sh"), template("guard.sh"));
  writeFileSync(
    join(dir, "guard.rules"),
    renderGuardRules(
      [
        { rule: rule("t/ask-push", "Pushing needs a look."), target: { target: "guard", decision: "ask", pattern: "git[[:space:]]+push" } },
        { rule: rule("t/no-force", "No force push."), target: { target: "guard", decision: "deny", pattern: "git[[:space:]]+push[[:space:]]+--force" } },
      ],
      "/",
    ),
  );

  test.each(shells.map((s) => [s.name, s.argv] as const))("on %s, deny wins over ask, ask applies alone, and no match prints nothing", (_n, argv) => {
    expect(guardDecision([...argv], dir, "git push --force")).toBe("deny");
    expect(guardDecision([...argv], dir, "git push")).toBe("ask");
    expect(guardDecision([...argv], dir, "git status")).toBe("");
  });

  test("the decision reason is forge:<rule-id> followed by the rule's description", () => {
    const r = sh("sh", [join(dir, "guard.sh")], { input: hookInput("git push --force") });
    expect(JSON.parse(r.stdout)).toEqual({
      hookSpecificOutput: { hookEventName: "PreToolUse", permissionDecision: "deny", permissionDecisionReason: "forge:t/no-force No force push." },
    });
  });

  test.each(shells.map((s) => [s.name, s.argv] as const))("on %s, a rule with a check is a candidate when its pattern matches; the check decides, and a later rule can still answer", (_n, argv) => {
    const d = mkdtempSync(join(tmpdir(), "forge-guard-check-"));
    writeFileSync(join(d, "guard.sh"), template("guard.sh"));
    writeFileSync(
      join(d, "guard.rules"),
      renderGuardRules(
        [
          { rule: rule("t/fetch", "Fetched packages need a look."), target: { target: "guard", decision: "ask", pattern: "npx", check: "ephemeral-exec" } },
          { rule: rule("t/any-npx", "Any npx."), target: { target: "guard", decision: "ask", pattern: "npx[[:space:]]+cowsay" } },
        ],
        "/",
      ),
    );
    const cwd = guardExampleDir({ "node_modules/.bin/tsc": "" });
    expect(guardDecision([...argv], d, "npx tsc --noEmit", cwd)).toBe("");
    expect(guardDecision([...argv], d, "npx cowsay", cwd)).toBe("ask");
    const r = sh(argv[0]!, [...argv.slice(1), join(d, "guard.sh")], { input: hookInput("npx some-typo", cwd), env: { FORGE_ASK_REVIEW: "never" } });
    expect(JSON.parse(r.stdout).hookSpecificOutput.permissionDecisionReason).toBe(
      `forge:t/fetch Fetched packages need a look. Here: some-typo is not installed where the command runs: no node_modules/.bin/some-typo from ${cwd} up to the repository root, so npx would fetch it from a registry and run it.`,
    );
  });

  test("a missing guard.sh makes the hook command fail, which onFailure: block turns into a blocked call", () => {
    expect(sh("sh", [join(dir, "missing.sh")], { input: hookInput("ls") }).status).not.toBe(0);
  });
});
