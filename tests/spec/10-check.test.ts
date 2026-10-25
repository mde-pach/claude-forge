/**
 * @spec 10 · check.sh, pre-commit and CI
 *
 * `.forge/check.sh [--changed | --all]` is the one entry point for the Stop
 * hook, the pre-commit hook and CI. It runs integrity, applied migrations, test
 * integrity, ast-grep `forbid` rules, format/lint/typecheck, migration checks and
 * tests, in that order, stops at the first failing step, and prints only
 * `forge:<rule-id>` lines. `--changed` covers staged, unstaged and untracked
 * files.
 */
import { beforeAll, describe, expect, test } from "bun:test";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, unlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { applied, commitAll, ensureTools, forge, git, hasBun, hasUv, linkTools, linkTs5, put, ROOT, sampleRepo, sh, TOOL_VERSIONS, TOOLS_CACHE } from "../helpers";
import { loadPacks } from "../../src/packs";
import { checkTools, renderChecks } from "../../src/render";
import { insertBlock, removeBlock } from "../../src/precommit";
import { runCheckExample } from "../../src/selftest";
import { parse as parseYaml } from "yaml";
import type { CheckTarget } from "../../src/types";

const T = 180_000;
const hasRuff = sh("sh", ["-c", "command -v ruff"]).status === 0;
let repo = "";

function check(mode = "--changed", env: Record<string, string> = {}) {
  return sh("sh", [".forge/check.sh", mode], { cwd: repo, env });
}
function reset() {
  git(repo, "reset", "-q", "--hard");
  git(repo, "clean", "-qfd", "-e", ".venv");
}
function stub(name: string, body: string) {
  const p = join(repo, ".venv", "bin", name);
  writeFileSync(p, `#!/bin/sh\n${body}\n`);
  chmodSync(p, 0o755);
}

beforeAll(() => {
  repo = applied("fastapi-app");
  commitAll(repo, "forge");
  mkdirSync(join(repo, ".venv", "bin"), { recursive: true });
  const ruff = sh("sh", ["-c", "command -v ruff"]).stdout.trim();
  if (ruff) symlinkSync(ruff, join(repo, ".venv", "bin", "ruff"));
}, T);

describe("steps", () => {
  test("a clean tree passes silently", () => {
    const r = check();
    expect(r.status).toBe(0);
    expect(r.stdout.split("\n").filter((l) => l && !l.includes("skipped"))).toEqual([]);
  }, T);

  test(
    "1. integrity: an owned file that differs from the lock fails, and so does a stray file in Forge's directories",
    () => {
      sh("sh", ["-c", "echo '# x' >> .forge/check.sh"], { cwd: repo });
      expect(check().stdout).toContain("forge:forge/integrity .forge/check.sh was modified (run forge diff)");
      reset();
      put(repo, ".forge/extra.txt", "x");
      expect(check().stdout).toContain("forge:forge/integrity .forge/extra.txt is not a forge file");
      reset();
    },
    T,
  );

  test(
    "2. applied migrations: a revision that exists at HEAD and is modified fails",
    () => {
      put(repo, "alembic/versions/0001_users.py", '"""users"""\n\nrevision = "0001"\ndown_revision = "x"\n');
      const r = check();
      expect(r.status).toBe(1);
      expect(r.stdout).toContain("forge:forge/applied-migration alembic/versions/0001_users.py exists at HEAD and was modified or deleted");
      reset();
    },
    T,
  );

  test(
    "3. tests: a test file deleted since HEAD fails",
    () => {
      unlinkSync(join(repo, "tests/test_main.py"));
      expect(check().stdout).toContain("forge:forge/deleted-test tests/test_main.py was deleted");
      reset();
    },
    T,
  );

  /** @rule python/pytest/no-fewer-asserts python/pytest/no-new-skip */
  test(
    "3. tests: fewer asserts than at HEAD fail (no-decrease), and a new skip fails even in a file new since HEAD (no-increase)",
    () => {
      put(repo, "tests/test_main.py", "from app.main import health\n\n\nasync def test_health() -> None:\n    await health()\n");
      put(repo, "tests/test_new.py", 'import pytest\n\n\n@pytest.mark.skip(reason="x")\ndef test_x() -> None:\n    assert True\n');
      const r = check();
      expect(r.status).toBe(1);
      expect(r.stdout).toContain("forge:python/pytest/no-fewer-asserts tests/test_main.py: 1 -> 0");
      expect(r.stdout).toContain("forge:python/pytest/no-new-skip tests/test_new.py: 0 -> 1");
      reset();
    },
    T,
  );

  /** @rule python/no-new-suppressions */
  test(
    "3. counts: a changed file with more suppression comments than in the base fails (grep no-increase); removing one passes",
    () => {
      put(repo, "app/main.py", readFileSync(join(repo, "app/main.py"), "utf8") + "\nX = f()  # type: ignore[name-defined]\n");
      const r = check();
      expect(r.status).toBe(1);
      expect(r.stdout).toContain("forge:python/no-new-suppressions app/main.py: 0 -> 1");
      reset();
      put(repo, "app/sup.py", "import os  # noqa: F401\n");
      git(repo, "add", "-A");
      git(repo, "commit", "-qm", "sup", "--no-verify");
      put(repo, "app/sup.py", "import os\n\nprint(os.sep)\n");
      expect(check().stdout).not.toContain("no-new-suppressions");
      git(repo, "reset", "-q", "--hard", "HEAD~1");
    },
    T,
  );

  /** @rule python/sqlalchemy/no-query-api */
  test(
    "4. ast-grep forbid rules report file:line:column with the rule's description",
    () => {
      put(repo, "app/repo.py", "def f(session):\n    return session.query(1)\n");
      const r = check();
      expect(r.status).toBe(1);
      expect(r.stdout).toContain("forge:python/sqlalchemy/no-query-api app/repo.py:2:12 No legacy session.query().");
      reset();
    },
    T,
  );

  /** @rule python/lint */
  test.if(hasRuff)(
    "5. lint runs the project's ruff on the changed files and prints the first 15 lines on failure",
    () => {
      put(repo, "app/l.py", "import os\n");
      const r = check();
      expect(r.stdout).toContain("forge:python/lint failed\napp/l.py:1:8: F401");
      reset();
    },
    T,
  );

  /** @rule python/typecheck */
  test(
    "a check whose tool is not installed prints skipped and does not fail",
    () => {
      put(repo, "app/x.py", "X = 1\n");
      const r = check();
      expect(r.stdout).toContain("forge:python/typecheck skipped: basedpyright not installed");
      reset();
    },
    T,
  );

  /** @rule python/sqlalchemy/alembic/check */
  test(
    "6. migration checks run only against a local database; a remote DATABASE_URL prints skipped and never blocks",
    () => {
      stub("alembic", 'echo "FAILED: Target database is not up to date."; exit 1');
      put(repo, "app/models/__init__.py", "from sqlalchemy.orm import DeclarativeBase\n\n\nclass Base(DeclarativeBase):\n    pass\n");
      const remote = check("--changed", { DATABASE_URL: "postgresql://u:p@db.prod.example.com/app" });
      expect(remote.stdout).toContain("forge:python/sqlalchemy/alembic/check skipped: no local database");
      const local = check("--changed", { DATABASE_URL: "postgresql://u:p@localhost/app" });
      expect(local.status).toBe(1);
      expect(local.stdout).toContain("forge:python/sqlalchemy/alembic/check failed\nFAILED: Target database is not up to date.");
      unlinkSync(join(repo, ".venv/bin/alembic"));
      reset();
    },
    T,
  );

  /** @rule python/pytest/run */
  test(
    "7. tests: with --changed, pytest gets the changed test files plus tests/**/test_<module>.py of each changed module",
    () => {
      stub("pytest", 'printf "ARGS:%s\\n" "$*"; exit 1');
      put(repo, "app/main.py", 'async def health() -> dict[str, str]:\n    return {"status": "ok"}\n');
      const r = check();
      expect(r.stdout).toContain("forge:python/pytest/run failed\nARGS:-q tests/test_main.py");
      unlinkSync(join(repo, ".venv/bin/pytest"));
      reset();
    },
    T,
  );

  /**
   * Claude Code ends a hook that runs past its timeout with SIGTERM. A shell
   * trap that only cleaned up let the script carry on without its temporary
   * directory, and Claude saw "cannot create …: Directory nonexistent".
   */
  test(
    "a terminated check.sh exits at once (143) instead of carrying on without its temporary directory",
    async () => {
      const started = join(repo, ".venv", "started");
      stub("pytest", `: > "${started}"; sleep 5; echo late; exit 1`);
      put(repo, "app/main.py", 'async def health() -> dict[str, str]:\n    return {"status": "ok"}\n');
      const p = Bun.spawn(["sh", ".forge/check.sh", "--changed"], { cwd: repo, stdout: "pipe", stderr: "pipe" });
      for (let i = 0; i < 600 && !existsSync(started); i++) await Bun.sleep(100);
      const t0 = performance.now();
      p.kill("SIGTERM");
      expect(await p.exited).toBe(143);
      expect(performance.now() - t0).toBeLessThan(3000);
      const stderr = await new Response(p.stderr).text();
      const stdout = await new Response(p.stdout).text();
      expect(stderr + stdout).not.toMatch(/nonexistent|No such file|cannot (create|open)/);
      expect(stdout).not.toContain("forge:python/pytest/run failed");
      unlinkSync(join(repo, ".venv/bin/pytest"));
      unlinkSync(started);
      reset();
    },
    T,
  );

  /** @rule python/pytest/run typescript/vitest/run */
  test(
    "a test run has a 300 s timeout; one that runs past its timeout is stopped and fails saying so first, then its output until then",
    () => {
      for (const rule of ["python/pytest/run", "typescript/vitest/run"]) {
        expect(readFileSync(join(import.meta.dir, `../../packs/${rule.replace("/run", "")}/rules/run.yaml`), "utf8")).toMatch(/timeout: 300\b/);
      }
      const r = engine(
        `mode=--all; forge_check_meta_1() { c_id=x/run; c_stage=test; c_tools=''; c_when=0; c_localdb=0; c_timeout=1; c_changed=0; c_files=0; c_fix=0; }
forge_check_run_1() { echo "collected 3 items"; sleep 5; echo finished; }
forge_run_check 1; echo "rc=$?"`,
      );
      expect(r.stdout).toStartWith("forge:x/run failed: timed out after 1s and was stopped");
      expect(r.stdout).toContain("collected 3 items\nrc=1");
      expect(r.stdout).not.toContain("finished");
    },
    T,
  );

  /**
   * Seen in better-auth: the first lines of a Vitest run were all Node's
   * `ExperimentalWarning: SQLite` notes, so the block reason never showed which
   * test failed or why (`ECONNREFUSED 127.0.0.1:5432`). Warnings are kept, after
   * the failure.
   */
  /** @rule typescript/vitest/run python/pytest/run */
  test.each([
    ["Vitest", VITEST_OUT, ["      Tests  1 failed | 27 passed (28)", " FAIL  |better-auth| src/plugins/oauth-proxy/oauth-proxy.test.ts > oauth-proxy > should work with database mode", "Error: connect ECONNREFUSED 127.0.0.1:5432", " ❯ getPostgres src/test-utils/test-instance.ts:68:4"]],
    ["pytest", PYTEST_OUT, ["FAILED tests/test_db.py::test_db - psycopg.OperationalError: connection refused", "E       psycopg.OperationalError: connection refused", "tests/test_db.py:5: OperationalError"]],
  ])(
    "a failing %s run shows the failure first (summary, failing tests and their errors), then the rest of its output, warnings included, within 25 lines",
    (_name, output, failure) => {
      const r = engine(
        `mode=--changed; forge_check_meta_1() { c_id=x/run; c_stage=test; c_tools=''; c_when=1; c_localdb=0; c_timeout=''; c_changed=0; c_files=0; c_fix=0; }
forge_when_1() { return 0; }; forge_scope_1() { return 0; }; printf 'x\\n' > "$FORGE_TMP/changed"; : > "$FORGE_TMP/existing"
forge_check_run_1() { printf '%s\\n' "$OUT"; return 1; }
forge_run_check 1`,
        { OUT: output },
      );
      const lines = r.stdout.trimEnd().split("\n");
      expect(lines[0]).toBe("forge:x/run failed");
      expect(lines.length).toBeLessThanOrEqual(26);
      const head = lines.slice(1, failure.length + 3).join("\n");
      for (const l of failure) expect(head).toContain(l);
      expect(r.stdout).toContain("--- output:\n");
      expect(r.stdout).toMatch(/(Experimental|Pytest)Warning/);
      expect(lines.at(-1)).toMatch(/^\.\.\. \(\d+ more lines\)$/);
    },
    T,
  );

  test(
    "output that fits is shown as it is; otherwise, without a known test format, the lines naming an error or a failure come first",
    () => {
      const r = engine(
        `printf 'a\\nb\\n' | forge_excerpt 5; i=0; while [ $i -lt 30 ]; do echo "progress $i: Warning: slow"; i=$((i + 1)); done > "$FORGE_TMP/o"; echo "fatal: Error: boom" >> "$FORGE_TMP/o"; forge_excerpt 5 < "$FORGE_TMP/o"`,
      );
      expect(r.stdout).toBe("a\nb\nfatal: Error: boom\n--- output:\nprogress 0: Warning: slow\nprogress 1: Warning: slow\n... (28 more lines)\n");
    },
    T,
  );

  /** @rule typescript/vitest/run python/pytest/run */
  test(
    "failing tests are read from Vitest (with workspace projects) and pytest output, with the files to run again; refused connections, failed DNS lookups and timeouts are named as environmental",
    () => {
      const out = `${VITEST_OUT}
⎯⎯⎯⎯⎯⎯ Failed Suites 1 ⎯⎯⎯⎯⎯⎯⎯

 FAIL  |@better-auth/api-key| src/api-key.test.ts [ src/api-key.test.ts ]
Error: connect ECONNREFUSED 127.0.0.1:5432
Caused by: Error: getaddrinfo EAI_AGAIN accounts.google.com
${PYTEST_OUT}
E   psycopg.OperationalError: connection to server at "127.0.0.1", port 6379 failed: Connection refused
ERROR tests/test_cache.py::test_get - TimeoutError
`;
      const r = engine(`printf '%s\\n' "$OUT" | forge_failed_tests > "$FORGE_TMP/ids"; cat "$FORGE_TMP/ids"; echo; forge_test_files < "$FORGE_TMP/ids"; echo; printf '%s\\n' "$OUT" | forge_env_errors`, { OUT: out });
      expect(r.stdout).toBe(
        [
          "tests/test_cache.py::test_get",
          "tests/test_db.py::test_db",
          "|@better-auth/api-key| src/api-key.test.ts",
          "|better-auth| src/plugins/oauth-proxy/oauth-proxy.test.ts > oauth-proxy > should work with database mode",
          "",
          "src/api-key.test.ts",
          "src/plugins/oauth-proxy/oauth-proxy.test.ts",
          "tests/test_cache.py",
          "tests/test_db.py",
          "",
          "Postgres refused the connection at 127.0.0.1:5432 (ECONNREFUSED)",
          "DNS lookup for accounts.google.com failed (EAI_AGAIN): no network?",
          "E       psycopg.OperationalError: connection refused",
          "Redis refused the connection on port 6379",
          "",
        ].join("\n"),
      );
    },
    T,
  );
});

const WARN = "(node:5155) ExperimentalWarning: SQLite is an experimental feature and might change at any time\n(Use `node --trace-warnings ...` to show where the warning was created)\n";
const VITEST_OUT = `
 RUN  v4.1.5 /work

${WARN.repeat(15)} ❯ |better-auth| src/plugins/oauth-proxy/oauth-proxy.test.ts (28 tests | 1 failed) 5487ms
       × should work with database mode 50ms

⎯⎯⎯⎯⎯⎯⎯ Failed Tests 1 ⎯⎯⎯⎯⎯⎯⎯

 FAIL  |better-auth| src/plugins/oauth-proxy/oauth-proxy.test.ts > oauth-proxy > should work with database mode
Error: connect ECONNREFUSED 127.0.0.1:5432
 ❯ ../../node_modules/.pnpm/pg-pool@3.12.0_pg@8.19.0/node_modules/pg-pool/index.js:45:11
 ❯ getPostgres src/test-utils/test-instance.ts:68:4
     66|   });
     67|   if (postgresSchema) {
     68|    await pool.query(
       |    ^
 ❯ getTestInstance src/test-utils/test-instance.ts:122:13

⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯[1/1]⎯


 Test Files  1 failed | 40 passed (41)
      Tests  1 failed | 27 passed (28)
   Start at  05:50:05
   Duration  9.99s
`;
const PYTEST_OUT = `============================= test session starts ==============================
collected 3 items

${"tests/test_db.py:1: PytestWarning: something is deprecated\n".repeat(25)}
=================================== FAILURES ===================================
___________________________________ test_db ____________________________________

    def test_db():
>       conn = psycopg.connect("postgresql://localhost:5432/app")
E       psycopg.OperationalError: connection refused

tests/test_db.py:5: OperationalError
=========================== short test summary info ============================
FAILED tests/test_db.py::test_db - psycopg.OperationalError: connection refused
========================= 1 failed, 2 passed in 0.12s ==========================
`;

/** Runs a snippet with lib.sh and engine.sh loaded, as check.sh has them, in a fresh temporary directory. */
function engine(snippet: string, env: Record<string, string> = {}, cwd = repo) {
  const dir = join(import.meta.dir, "../../templates/scripts");
  const tmp = mkdtempSync(join(tmpdir(), "forge-engine-"));
  return sh("sh", ["-c", `IFS='\n'\nFORGE_TMP=${tmp}\n${readFileSync(join(dir, "lib.sh"), "utf8")}\n${readFileSync(join(dir, "engine.sh"), "utf8")}\n${snippet}`], { cwd, env });
}

describe("local database detection", () => {
  test.each([
    ["postgresql://u:p@localhost:5432/db", true],
    ["postgresql+asyncpg://u:p@127.0.0.1/db", true],
    ["postgres://u@[::1]:5432/db", true],
    ["postgresql:///db", true],
    ["postgresql://u@/db?host=/var/run/postgresql", true],
    ["postgresql://u:p@db.example.com:5432/db", false],
    ["postgresql://u:p@10.0.0.5/db", false],
  ])("DATABASE_URL=%s is local: %p", (url, local) => {
    const engine = readFileSync(join(import.meta.dir, "../../templates/scripts/engine.sh"), "utf8");
    const r = sh("sh", ["-c", `tool() { :; }; ${engine}\nforge_local_db && echo local || echo remote`], { env: { DATABASE_URL: url }, cwd: "/" });
    expect(r.stdout.trim()).toBe(local ? "local" : "remote");
  });
});

/** What installing a hook manager in a clone does to git: its hook in .git/hooks, or Husky's core.hooksPath. */
function installManager(r: string, file: string): void {
  if (file === ".husky/pre-commit") git(r, "config", "core.hooksPath", ".husky/_");
  else if (file.startsWith("lefthook")) put(r, ".git/hooks/pre-commit", "#!/bin/sh\n# lefthook\nlefthook run pre-commit \"$@\"\n");
  else put(r, ".git/hooks/pre-commit", "#!/usr/bin/env bash\n# File generated by pre-commit: https://pre-commit.com\n");
}

describe("pre-commit", () => {
  /** @rule typescript/no-explicit-any */
  test(
    "apply --yes creates .git/hooks/pre-commit running check.sh --changed, which blocks a bad commit",
    () => {
      const r = applied("eslint-app");
      expect(readFileSync(join(r, ".git/hooks/pre-commit"), "utf8")).toContain("exec sh .forge/check.sh --changed");
      commitAll(r, "forge");
      put(r, "src/bad.ts", "export const x = 1 as any;\n");
      git(r, "add", "-A");
      const c = git(r, "commit", "-qm", "bad");
      expect(c.status).not.toBe(0);
      expect(c.stdout + c.stderr).toContain("forge:typescript/no-explicit-any src/bad.ts");
    },
    T,
  );

  /**
   * With a hook manager, Forge adds its check to the manager's config as a block
   * between `# forge: begin` and `# forge: end`, recorded in the lock's `blocks`:
   * the rest of the file is left as it was, and `remove` takes the block out.
   */
  test.each([
    [".pre-commit-config.yaml", "repos:\n  - repo: https://github.com/astral-sh/ruff-pre-commit\n    rev: v0.6.0\n    hooks:\n      - id: ruff\n", "entry: sh .forge/check.sh --changed", "pre-commit install"],
    ["lefthook.yml", "pre-commit:\n  commands:\n    lint:\n      run: npm run lint\n", "run: sh .forge/check.sh --changed", "lefthook install"],
    [".husky/pre-commit", "npx lint-staged\n", "sh .forge/check.sh --changed || exit 1", "npx husky"],
  ])(
    "with %s, apply --pre-commit adds the check as a marked block, once; status shows it, and that it runs once the manager is installed; remove takes it out",
    (file, mine, line, activate) => {
      const r = sampleRepo("eslint-app");
      put(r, file, mine);
      commitAll(r, "manager");
      expect(forge(["apply", "--yes", "--pre-commit"], r).status).toBe(0);
      expect(forge(["apply", "--yes"], r).stdout).toContain("forge: up to date");
      const text = readFileSync(join(r, file), "utf8");
      expect(text.split(line).length).toBe(2);
      expect(text).toContain("# forge: begin (forge remove deletes this block)");
      expect(text.replace(/[ \t]*# forge: begin[^]*# forge: end\n/, "")).toBe(mine);
      expect(existsSync(join(r, ".git/hooks/pre-commit"))).toBe(false);
      expect(JSON.parse(readFileSync(join(r, ".forge/forge.lock.json"), "utf8")).blocks).toEqual([expect.objectContaining({ file, created: false })]);
      // the manager is not installed in this fresh clone: git does not run its config
      expect(forge(["status"], r).stdout).toContain(`Pre-commit: ${file} runs the check (forge), but ${file.replace(/\/pre-commit$/, "")} is not installed in this clone, so commits are not checked. To activate: ${activate}`);
      expect(JSON.parse(forge(["status", "--json"], r).stdout).pre_commit).toEqual({ installed: false, file, forge: true, activate: expect.stringContaining(activate) });
      installManager(r, file);
      expect(forge(["status"], r).stdout).toContain(`Pre-commit: ${file} (forge)\n`);
      git(r, "config", "--unset", "core.hooksPath");
      rmSync(join(r, ".git/hooks/pre-commit"), { force: true });
      expect(forge(["remove", "--yes"], r).status).toBe(0);
      expect(git(r, "status", "--porcelain", "--untracked-files=all").stdout).toBe("");
    },
    T,
  );

  test(
    "with core.hooksPath in the repository, the check goes into its pre-commit hook, which then blocks a bad commit",
    () => {
      const r = sampleRepo("eslint-app");
      git(r, "config", "core.hooksPath", "githooks");
      expect(forge(["apply", "--yes", "--pre-commit"], r).status).toBe(0);
      expect(existsSync(join(r, ".git/hooks/pre-commit"))).toBe(false);
      expect(readFileSync(join(r, "githooks/pre-commit"), "utf8")).toContain("sh .forge/check.sh --changed || exit 1");
      commitAll(r, "forge");
      put(r, "src/bad.ts", "export const x = 1 as any;\n");
      git(r, "add", "-A");
      const c = git(r, "commit", "-qm", "bad");
      expect(c.status).not.toBe(0);
      expect(c.stdout + c.stderr).toContain("forge:typescript/no-explicit-any src/bad.ts");
    },
    T,
  );

  test(
    "a block the user deleted is reported by status and added back by apply",
    () => {
      const r = sampleRepo("eslint-app");
      put(r, "lefthook.yml", "pre-push:\n  commands:\n    t:\n      run: x\n");
      expect(forge(["apply", "--yes", "--pre-commit"], r).status).toBe(0);
      put(r, "lefthook.yml", "pre-push:\n  commands:\n    t:\n      run: x\n");
      const st = forge(["status"], r).stdout;
      expect(st).toContain("lefthook.yml: forge's pre-commit block is missing (apply adds it back)");
      expect(st).toContain("Pre-commit: NOT INSTALLED, so commits are not checked");
      expect(forge(["apply", "--yes"], r).status).toBe(0);
      expect(readFileSync(join(r, "lefthook.yml"), "utf8")).toContain("run: sh .forge/check.sh --changed");
    },
    T,
  );

  test(
    "a config Forge cannot edit safely (lefthook jobs) is left alone, and apply and status warn that commits are not checked",
    () => {
      const r = sampleRepo("eslint-app");
      put(r, "lefthook.yml", "pre-commit:\n  jobs:\n    - run: npm run lint\n");
      const a = forge(["apply", "--yes", "--pre-commit"], r);
      expect(readFileSync(join(r, "lefthook.yml"), "utf8")).toBe("pre-commit:\n  jobs:\n    - run: npm run lint\n");
      expect(a.stdout).toContain('forge: pre-commit: not installed, so commits are not checked; add "sh .forge/check.sh --changed" to lefthook.yml');
      expect(forge(["status"], r).stdout).toContain('Pre-commit: NOT INSTALLED, so commits are not checked. To install: add "sh .forge/check.sh --changed" to lefthook.yml');
      expect(JSON.parse(forge(["status", "--json"], r).stdout).pre_commit).toEqual({ installed: false });
    },
    T,
  );

  test(
    "a hook manager that already runs .forge/check.sh counts as installed, and is not edited",
    () => {
      const r = sampleRepo("eslint-app");
      const mine = "pre-commit:\n  commands:\n    forge:\n      run: sh .forge/check.sh --changed\n";
      put(r, "lefthook.yml", mine);
      expect(forge(["apply", "--yes"], r).status).toBe(0);
      expect(readFileSync(join(r, "lefthook.yml"), "utf8")).toBe(mine);
      installManager(r, "lefthook.yml");
      expect(forge(["status"], r).stdout).toContain("Pre-commit: lefthook.yml\n");
    },
    T,
  );

  /**
   * A hook manager's config is the team's file, usually tracked: `--yes` (scripts,
   * CI) confirms settings changes and creates `.git/hooks/pre-commit`, but only
   * `--pre-commit` (or a yes at the prompt) edits the manager's config.
   */
  test(
    "apply --yes alone leaves a hook manager's config as it is, and says how to add the check",
    () => {
      const r = sampleRepo("eslint-app");
      put(r, ".husky/pre-commit", "npx lint-staged\n");
      commitAll(r, "manager");
      const a = forge(["apply", "--yes"], r);
      expect(a.status).toBe(0);
      expect(readFileSync(join(r, ".husky/pre-commit"), "utf8")).toBe("npx lint-staged\n");
      expect(a.stdout).toContain('forge: pre-commit: not installed, so commits are not checked; run forge apply interactively (or with --pre-commit) to add "sh .forge/check.sh --changed" to .husky/pre-commit');
      expect(git(r, "status", "--porcelain", ".husky").stdout).toBe("");
    },
    T,
  );

  test(
    "remove leaves a block whose end marker was deleted (and the user's lines after it), and exits 1",
    () => {
      const r = sampleRepo("eslint-app");
      put(r, ".husky/pre-commit", "npx lint-staged\n");
      commitAll(r, "manager");
      expect(forge(["apply", "--yes", "--pre-commit"], r).status).toBe(0);
      const broken = readFileSync(join(r, ".husky/pre-commit"), "utf8").replace("# forge: end\n", "") + "echo important\n";
      put(r, ".husky/pre-commit", broken);
      const rm = forge(["remove", "--yes"], r);
      expect(rm.status).toBe(1);
      expect(rm.stderr).toContain("forge: .husky/pre-commit: forge's pre-commit block cannot be removed safely");
      expect(readFileSync(join(r, ".husky/pre-commit"), "utf8")).toBe(broken);
    },
    T,
  );

  /**
   * In a config Forge created, its block holds the parent keys (`repos:`,
   * `pre-commit:` / `commands:`); entries the user added below the block belong
   * to them, so remove keeps those keys. A removal that would change anything
   * else in a YAML config is refused.
   */
  test.each([
    ["pre-commit", "- repo: https://github.com/astral-sh/ruff-pre-commit\n  rev: v0.6.0\n  hooks:\n    - id: ruff\n", { repos: [{ repo: "https://github.com/astral-sh/ruff-pre-commit", rev: "v0.6.0", hooks: [{ id: "ruff" }] }] }],
    ["lefthook", "    lint:\n      run: eslint\n", { "pre-commit": { commands: { lint: { run: "eslint" } } } }],
  ] as const)("%s: entries added under the parent keys Forge created survive remove", (kind, added, expected) => {
    const created = insertBlock(kind, undefined)!;
    const after = removeBlock(kind, created + added);
    expect(after).toBeDefined();
    expect(parseYaml(after!)).toEqual(expected);
    expect(removeBlock(kind, created)!.trim()).toBe("");
    // an entry added inside the block cannot be removed with it
    const inside = created.replace("# forge: end", "    - id: mine\n# forge: end");
    if (kind === "pre-commit") expect(removeBlock(kind, inside)).toBeUndefined();
  });

  test("a shell hook gets the check before the first exec/exit at the top level, not one inside an if, case or function", () => {
    const at = (text: string) => insertBlock("husky", text)!.split("\n").indexOf("sh .forge/check.sh --changed || exit 1");
    expect(at("if [ -n \"$CI\" ]; then\nexit 0\nfi\nnpx lint-staged\n")).toBe(5);
    expect(at("case $1 in\n  x) exit 0 ;;\nesac\nf() {\nexit 1\n}\nexec npx lint-staged\n")).toBe(7);
    expect(at("echo 'if you see this'\nexit 0\n")).toBe(2);
  });

  test("a CRLF config keeps CRLF line ends, and remove restores it exactly", () => {
    const text = "repos:\r\n  - repo: local\r\n    hooks:\r\n      - id: a\r\n        name: a\r\n        entry: a\r\n        language: system\r\n";
    const after = insertBlock("pre-commit", text)!;
    expect(after.replace(/\r\n/g, "")).not.toContain("\n");
    expect(removeBlock("pre-commit", after)).toBe(text);
    const hook = "npx lint-staged\r\n";
    expect(removeBlock("husky", insertBlock("husky", hook)!)).toBe(hook);
  });

  test(
    "an existing pre-commit hook is never touched, by apply or by remove",
    () => {
      const r = sampleRepo("eslint-app");
      put(r, ".git/hooks/pre-commit", "#!/bin/sh\necho mine\n");
      expect(forge(["apply", "--yes"], r).status).toBe(0);
      expect(forge(["remove", "--yes"], r).status).toBe(0);
      expect(readFileSync(join(r, ".git/hooks/pre-commit"), "utf8")).toBe("#!/bin/sh\necho mine\n");
      expect(forge(["status"], r).stdout).toContain('Pre-commit: NOT INSTALLED, so commits are not checked. To install: add "sh .forge/check.sh --changed" to .git/hooks/pre-commit');
    },
    T,
  );
});

/** @rule python/sqlalchemy/no-query-api */
test(
  "CI: --all checks the whole project, not only the changed files",
  () => {
    put(repo, "app/legacy.py", "def f(session):\n    return session.query(1)\n");
    git(repo, "add", "-A");
    git(repo, "commit", "-qm", "legacy", "--no-verify"); // the forge pre-commit hook would refuse it
    expect(check("--changed").stdout).not.toContain("legacy.py");
    expect(check("--all").stdout).toContain("forge:python/sqlalchemy/no-query-api app/legacy.py:2:12");
    git(repo, "reset", "-q", "--hard", "HEAD~1");
  },
  T,
);

/**
 * A whole-project typecheck or lint run under `--changed` (Stop, pre-commit)
 * fails only when an error names a file in the change: errors already in
 * unchanged files are not Claude's, and blocking on them would block every
 * turn. `--all` (CI) still fails on any error.
 */
describe("errors in unchanged files", () => {
  /** A TypeScript repository with a type error committed in src/legacy.ts. */
  function legacyRepo(): string {
    ensureTools();
    const r = sampleRepo("eslint-app");
    linkTools(r, "node");
    put(r, "src/legacy.ts", 'export const legacy: number = "oops";\n');
    commitAll(r, "legacy");
    expect(forge(["apply", "--yes"], r).status).toBe(0);
    commitAll(r, "forge");
    return r;
  }

  /** @rule typescript/typecheck */
  test.if(hasBun)(
    "a clean change passes even though an unchanged file has a type error, and says so",
    () => {
      const r = legacyRepo();
      put(r, "src/ok.ts", "export const ok: number = 1;\n");
      const c = sh("sh", [".forge/check.sh", "--changed"], { cwd: r });
      expect(c.status).toBe(0);
      expect(c.stdout).toContain("forge:typescript/typecheck ok for the changed files (errors remain in unchanged files");
    },
    T,
  );

  /** @rule typescript/typecheck */
  test.if(hasBun)(
    "a type error in a changed file fails, and only the changed file's diagnostics are shown",
    () => {
      const r = legacyRepo();
      put(r, "src/ok.ts", 'export const ok: number = "no";\n');
      const c = sh("sh", [".forge/check.sh", "--changed"], { cwd: r });
      expect(c.status).toBe(1);
      expect(c.stdout).toContain("forge:typescript/typecheck failed\nsrc/ok.ts(1,14): error TS2322");
      expect(c.stdout).not.toContain("src/legacy.ts");
    },
    T,
  );

  /**
   * tsc 5.x with `"pretty": true` colours its diagnostics even when they go to a file
   * (`\e[96msrc/legacy.ts\e[0m:\e[93m1\e[0m:...`); tsc 7 does not. Run with the real tsc 5.
   */
  /** @rule typescript/typecheck */
  test.if(hasBun)(
    "with tsc 5 and \"pretty\": true (coloured diagnostics), an unchanged file's error still does not block",
    () => {
      const r = legacyRepo();
      linkTs5(r);
      put(r, "tsconfig.json", '{"compilerOptions": {"target": "ES2022", "module": "ESNext", "strict": true, "noEmit": true, "pretty": true}, "include": ["src"]}\n');
      commitAll(r, "pretty");
      const v = sh("node_modules/.bin/tsc", ["-v"], { cwd: r });
      expect(v.stdout).toContain(`Version ${TOOL_VERSIONS.typescript5}`);
      put(r, "src/ok.ts", "export const ok: number = 1;\n");
      const ok = sh("sh", [".forge/check.sh", "--changed"], { cwd: r });
      expect(ok.stdout).toContain("forge:typescript/typecheck ok for the changed files (errors remain in unchanged files");
      expect(ok.status).toBe(0);
      put(r, "src/ok.ts", 'export const ok: number = "no";\n');
      const bad = sh("sh", [".forge/check.sh", "--changed"], { cwd: r });
      expect(bad.status).toBe(1);
      expect(bad.stdout).toContain("forge:typescript/typecheck failed");
      expect(bad.stdout).toContain("src/ok.ts");
      expect(bad.stdout).not.toContain("src/legacy.ts");
    },
    T,
  );

  /** @rule typescript/typecheck */
  test.if(hasBun)(
    "--all still fails on the error in the unchanged file",
    () => {
      const c = sh("sh", [".forge/check.sh", "--all"], { cwd: legacyRepo() });
      expect(c.status).toBe(1);
      expect(c.stdout).toContain("forge:typescript/typecheck failed\nsrc/legacy.ts(1,14): error TS2322");
    },
    T,
  );

  /**
   * tsc runs with `--pretty false` (plain diagnostics whatever the tsconfig says) and
   * `--incremental`, its build info kept in `.forge/state/` (ignored by git): a
   * project's own `tsBuildInfoFile`, or a composite project's build info used by
   * `tsc -b`, is never written.
   */
  /** @rule typescript/typecheck */
  test.if(hasBun)(
    "tsc runs incrementally with plain output, its build info under .forge/state/ and never in the project, also for a composite project",
    () => {
      const r = legacyRepo();
      linkTs5(r);
      put(r, "tsconfig.json", '{"compilerOptions": {"target": "ES2022", "module": "ESNext", "strict": true, "composite": true, "outDir": "out", "pretty": true}, "include": ["src"]}\n');
      commitAll(r, "composite");
      put(r, "src/ok.ts", 'export const ok: number = "no";\n');
      const first = sh("sh", [".forge/check.sh", "--changed"], { cwd: r });
      expect(first.stdout).toContain("forge:typescript/typecheck failed\nsrc/ok.ts(1,14): error TS2322");
      expect(first.stdout).not.toContain("\u001b[");
      expect(existsSync(join(r, ".forge/state/tsc.tsbuildinfo"))).toBe(true);
      expect(existsSync(join(r, "out"))).toBe(false);
      expect(git(r, "status", "--porcelain").stdout.trim()).toBe("?? src/ok.ts");
      const second = sh("sh", [".forge/check.sh", "--changed"], { cwd: r });
      expect(second.stdout).toBe(first.stdout);
    },
    T,
  );

  test.if(hasUv)(
    "the same holds for a lint check run on the whole project",
    () => {
      ensureTools();
      const r = sampleRepo("fastapi-app");
      linkTools(r, "python");
      put(r, ".forge/rules/lint-all.yaml", "id: local/lint-all\ndescription: ruff on the whole project.\nscope: {language: python}\nenforce:\n  - {target: check, stage: lint, run: \"ruff check --output-format concise .\"}\n");
      put(r, "app/legacy.py", "import os\n");
      commitAll(r, "legacy");
      expect(forge(["apply", "--yes"], r).status).toBe(0);
      commitAll(r, "forge");
      put(r, "app/new.py", "X = 1\n");
      const ok = sh("sh", [".forge/check.sh", "--changed"], { cwd: r });
      expect(ok.stdout).toContain("forge:local/lint-all ok for the added lines");
      expect(ok.stdout).not.toContain("forge:local/lint-all failed");
      put(r, "app/new.py", "import sys\n");
      const bad = sh("sh", [".forge/check.sh", "--changed"], { cwd: r });
      expect(bad.stdout).toContain("forge:local/lint-all failed\napp/new.py:1:8: F401");
      expect(bad.stdout).not.toContain("app/legacy.py");
    },
    T,
  );
});


/**
 * In a monorepo whose TypeScript config lives in a workspace package
 * (`packages/core/tsconfig.json`, `biome.json`), the checks run in that package,
 * with the tools installed there (pnpm) or hoisted to the root (npm, Yarn, Bun).
 * Diagnostics name repository paths, so they are attributed as at the root.
 */
describe("workspace packages", () => {
  /** The pnpm workspace sample with a type error committed in packages/core/src/legacy.ts; tools in the package or at the root. */
  function workspaceRepo(tools: "package" | "root"): string {
    ensureTools();
    const r = sampleRepo("pnpm-workspace-app");
    linkTools(tools === "package" ? join(r, "packages/core") : r, "node");
    put(r, "packages/core/src/legacy.ts", 'export const legacy: number = "oops";\n');
    commitAll(r, "legacy");
    expect(forge(["apply", "--yes"], r).status).toBe(0);
    commitAll(r, "forge");
    return r;
  }

  /** @rule typescript/typecheck */
  test.if(hasBun)(
    "the type check runs in the package: an error in a changed file fails with its repository path, one in an unchanged file does not block",
    () => {
      const r = workspaceRepo("package");
      put(r, "packages/core/src/ok.ts", "export const ok: number = 1;\n");
      const ok = sh("sh", [".forge/check.sh", "--changed"], { cwd: r });
      expect(ok.stdout).toContain("forge:typescript/typecheck ok for the changed files (errors remain in unchanged files");
      expect(ok.stdout).not.toContain("tsc (packages/core) not installed");
      put(r, "packages/core/src/ok.ts", 'export const ok: number = "no";\n');
      const bad = sh("sh", [".forge/check.sh", "--changed"], { cwd: r });
      expect(bad.status).toBe(1);
      expect(bad.stdout).toContain("forge:typescript/typecheck failed\npackages/core/src/ok.ts(1,14): error TS2322");
      expect(bad.stdout).not.toContain("legacy.ts");
    },
    T,
  );

  /** @rule typescript/format-lint */
  test.if(hasBun)(
    "Biome runs in the package with its config, on the changed files, and its findings name repository paths",
    () => {
      const r = workspaceRepo("package");
      put(r, "packages/core/src/ok.ts", "export const ok: any = 1;\n");
      const c = sh("sh", [".forge/check.sh", "--changed"], { cwd: r });
      expect(c.status).toBe(1);
      expect(c.stdout).toContain("forge:typescript/format-lint failed\npackages/core/src/ok.ts:1:18 lint/suspicious/noExplicitAny");
    },
    T,
  );

  /** @rule typescript/typecheck */
  test.if(hasBun)(
    "tools hoisted to the root's node_modules are used in the package",
    () => {
      const r = workspaceRepo("root");
      put(r, "packages/core/src/ok.ts", 'export const ok: number = "no";\n');
      const c = sh("sh", [".forge/check.sh", "--changed"], { cwd: r });
      expect(c.stdout).toContain("forge:typescript/typecheck failed\npackages/core/src/ok.ts(1,14): error TS2322");
      const status = forge(["status"], r).stdout;
      expect(status).not.toContain("tsc (packages/core)");
      expect(status).not.toContain("biome (packages/core)");
    },
    T,
  );
});

/**
 * Outside `--all`, a forbid rule or a lint finding counts only on a line the
 * change added (`git diff -U0` against the base; an edited line counts, moved code
 * counts, a new file counts fully): code that was already there is not this
 * change's. `--since <ref>` measures the change from `git merge-base <ref> HEAD`.
 */
describe("added lines", () => {
  const QUERY = "def f(session):\n    return session.query(1)\n";
  /** The fastapi sample, applied, with app/legacy.py (a forbidden match) committed. */
  function legacyRepo(): string {
    const r = applied("fastapi-app");
    put(r, "app/legacy.py", QUERY);
    git(r, "add", "-A");
    git(r, "commit", "-qm", "legacy", "--no-verify"); // the forge pre-commit hook would refuse it
    return r;
  }
  const run = (r: string, ...args: string[]) => sh("sh", [".forge/check.sh", ...args], { cwd: r });

  /** @rule python/sqlalchemy/no-query-api */
  test(
    "--changed: a match on a line that was already at HEAD does not fail; one on an added line does, and only that one is reported",
    () => {
      const r = legacyRepo();
      put(r, "app/legacy.py", QUERY + "\n\nX = 1\n");
      expect(run(r, "--changed").status).toBe(0);
      put(r, "app/legacy.py", QUERY + "\n\ndef g(session):\n    return session.query(2)\n");
      const out = run(r, "--changed");
      expect(out.status).toBe(1);
      expect(out.stdout).toContain("forge:python/sqlalchemy/no-query-api app/legacy.py:6:12");
      expect(out.stdout).not.toContain("app/legacy.py:2:12");
    },
    T,
  );

  /** @rule python/sqlalchemy/no-query-api */
  test(
    "a match spanning an edited line counts, and so does code moved to another place",
    () => {
      const r = applied("fastapi-app");
      put(r, "app/span.py", "def f(session):\n    return session.query(\n        1,\n    )\n\n\nX = 1\n");
      git(r, "add", "-A");
      git(r, "commit", "-qm", "span", "--no-verify");
      put(r, "app/span.py", "def f(session):\n    return session.query(\n        2,\n    )\n\n\nX = 1\n");
      expect(run(r, "--changed").stdout).toContain("forge:python/sqlalchemy/no-query-api app/span.py:2:12");
      git(r, "checkout", "-q", "--", "app/span.py");
      put(r, "app/span.py", "X = 1\n");
      put(r, "app/moved.py", "def f(session):\n    return session.query(\n        1,\n    )\n");
      expect(run(r, "--changed").stdout).toContain("forge:python/sqlalchemy/no-query-api app/moved.py:2:12");
    },
    T,
  );

  /** @rule python/sqlalchemy/no-query-api */
  test(
    "--all still reports every match in the project",
    () => {
      const r = legacyRepo();
      expect(run(r, "--all").stdout).toContain("forge:python/sqlalchemy/no-query-api app/legacy.py:2:12");
    },
    T,
  );

  /** @rule python/sqlalchemy/no-query-api */
  test(
    "--since <ref>: what a branch added since its merge base with <ref> fails, committed or not; the base's own code does not",
    () => {
      const r = legacyRepo();
      git(r, "checkout", "-q", "-b", "feature");
      put(r, "app/feature.py", "def g(session):\n    return session.query(2)\n");
      git(r, "add", "-A");
      git(r, "commit", "-qm", "feature", "--no-verify");
      const out = run(r, "--since", "main");
      expect(out.status).toBe(1);
      expect(out.stdout).toContain("forge:python/sqlalchemy/no-query-api app/feature.py:2:12");
      expect(out.stdout).not.toContain("app/legacy.py");
      // --changed sees nothing: the work is committed
      expect(run(r, "--changed").status).toBe(0);
    },
    T,
  );

  test(
    "--since a ref that has no merge base with HEAD fails with what to fetch",
    () => {
      const r = legacyRepo();
      const out = run(r, "--since", "origin/main");
      expect(out.status).toBe(1);
      expect(out.stdout).toContain("forge:forge/since no merge base for origin/main and HEAD");
    },
    T,
  );

  test(
    "--since: a migration that exists at the merge base and was modified on the branch fails",
    () => {
      const r = legacyRepo();
      git(r, "checkout", "-q", "-b", "feature");
      put(r, "alembic/versions/0001_users.py", '"""users"""\n\nrevision = "0001"\ndown_revision = "x"\n');
      git(r, "add", "-A");
      git(r, "commit", "-qm", "edit", "--no-verify");
      expect(run(r, "--since", "main").stdout).toContain(
        "forge:forge/applied-migration alembic/versions/0001_users.py exists at the merge base with main and was modified or deleted",
      );
    },
    T,
  );

  /** @rule python/lint */
  test.if(hasUv)(
    "a lint finding on a line that was already there does not fail; one on an added line does",
    () => {
      ensureTools();
      const r = sampleRepo("fastapi-app");
      linkTools(r, "python");
      put(r, "app/legacy.py", "import os\n");
      commitAll(r, "legacy");
      expect(forge(["apply", "--yes"], r).status).toBe(0);
      commitAll(r, "forge");
      put(r, "app/legacy.py", "import os\n\nX = 1\n");
      const ok = run(r, "--changed");
      expect(ok.stdout).toContain("forge:python/lint ok for the added lines");
      expect(ok.stdout).not.toContain("forge:python/lint failed");
      put(r, "app/legacy.py", "import os\nimport sys\n\nX = 1\n");
      const bad = run(r, "--changed");
      expect(bad.status).toBe(1);
      expect(bad.stdout).toContain("forge:python/lint failed\napp/legacy.py:2:8: F401");
      expect(bad.stdout).not.toContain("app/legacy.py:1:8");
    },
    T,
  );
});

/**
 * The config ratchet compares the changed tool configs with the base and fails
 * when one was loosened, for Claude at Stop and for anyone at pre-commit and in
 * CI, until the change is accepted in forge.yaml.
 */
describe("config ratchet", () => {
  /** @rule typescript/config-ratchet */
  test.if(hasBun)(
    "lowering a Biome rule fails pre-commit with the setting, before and after; the same line accepted in forge.yaml passes",
    () => {
      const r = sampleRepo("nextjs-prisma-app");
      put(r, "forge.yaml", 'forge: "0.5"\ndisable:\n  - id: typescript/bun/lock-sync\n    reason: the sample lock file is a stub\n');
      commitAll(r, "config");
      expect(forge(["apply", "--yes"], r).status).toBe(0);
      commitAll(r, "forge");
      const biome = JSON.parse(readFileSync(join(r, "biome.json"), "utf8"));
      biome.linter.rules.suspicious = { ...(biome.linter.rules.suspicious ?? {}), noExplicitAny: "off" };
      put(r, "biome.json", JSON.stringify(biome, null, 2));
      const out = sh("sh", [".forge/check.sh", "--changed"], { cwd: r });
      expect(out.status).toBe(1);
      expect(out.stdout).toContain("forge:typescript/config-ratchet failed\nbiome.json: linter.rules.suspicious.noExplicitAny error -> off");
      put(r, "forge.yaml", readFileSync(join(r, "forge.yaml"), "utf8") + 'accept:\n  - rule: typescript/config-ratchet\n    change: "biome.json: linter.rules.suspicious.noExplicitAny error -> off"\n    reason: migrating a legacy module\n');
      expect(sh("sh", [".forge/check.sh", "--changed"], { cwd: r }).stdout).not.toContain("config-ratchet");
    },
    T,
  );

  /** @rule python/config-ratchet */
  test.if(hasUv)(
    "with the project's Python: a ruff ignore added to pyproject.toml fails; a dependency change does not",
    () => {
      ensureTools();
      const r = sampleRepo("fastapi-app");
      linkTools(r, "python");
      put(r, "forge.yaml", 'forge: "0.5"\ndisable:\n  - id: python/uv/lock-sync\n    reason: the sample lock file is a stub\n');
      commitAll(r, "config");
      expect(forge(["apply", "--yes"], r).status).toBe(0);
      commitAll(r, "forge");
      const py = readFileSync(join(r, "pyproject.toml"), "utf8");
      put(r, "pyproject.toml", py.replace('dependencies = [', 'dependencies = ["httpx>=0.28", '));
      expect(sh("sh", [".forge/check.sh", "--changed"], { cwd: r }).stdout).not.toContain("config-ratchet");
      put(r, "pyproject.toml", py + '\n[tool.ruff.lint.per-file-ignores]\n"app/*" = ["E722"]\n');
      const out = sh("sh", [".forge/check.sh", "--changed"], { cwd: r });
      expect(out.stdout).toContain("forge:python/config-ratchet failed\npyproject.toml: tool.ruff.lint.per-file-ignores adds E722 for app/*");
    },
    T,
  );

  /** @rule python/config-ratchet */
  test(
    "without the project's Python the ratchet is skipped, never passed silently",
    () => {
      const r = applied("fastapi-app");
      commitAll(r, "forge");
      put(r, "pyproject.toml", readFileSync(join(r, "pyproject.toml"), "utf8") + "\n# edited\n");
      expect(sh("sh", [".forge/check.sh", "--changed"], { cwd: r }).stdout).toContain("forge:python/config-ratchet skipped: python not installed");
    },
    T,
  );
});

/**
 * The Docker sample (a multi-stage Dockerfile, Compose with a database and a
 * GitHub Actions workflow) passes every docker, docker/compose and
 * github-actions rule; a typical slip in each file fails.
 */
describe("docker and github-actions", () => {
  function dockerRepo(): string {
    const r = applied("docker-app");
    commitAll(r, "forge");
    return r;
  }
  const run = (r: string, mode = "--changed") => sh("sh", [".forge/check.sh", mode], { cwd: r });

  /** @pack docker docker/compose github-actions */
  test(
    "the sample passes as committed (--all)",
    () => {
      const out = run(dockerRepo(), "--all").stdout;
      expect(out).not.toMatch(/forge:(docker|github-actions)\S* (failed|\S+:\d)/);
    },
    T,
  );

  /** @pack docker */
  test(
    "forge status says how to let Claude run docker outside the sandbox (sandbox.excludedCommands), without writing it",
    () => {
      const r = dockerRepo();
      expect(forge(["status"], r).stdout).toContain('"sandbox": {"excludedCommands": ["docker compose *"]}');
      expect(readFileSync(join(r, ".claude/settings.json"), "utf8")).not.toContain("excludedCommands");
    },
    T,
  );

  /** @rule docker/compose/datastore-loopback docker/non-root-user github-actions/zizmor */
  test(
    "publishing Postgres on every interface, dropping USER, and an unpinned action each fail",
    () => {
      const r = dockerRepo();
      const compose = readFileSync(join(r, "compose.yaml"), "utf8");
      put(r, "compose.yaml", compose.replace('"127.0.0.1:5432:5432"', '"5432:5432"'));
      expect(run(r).stdout).toContain("forge:docker/compose/datastore-loopback compose.yaml:8:9");
      // forbid rules run before the lint stage; with compose.yaml restored, the lint-stage checks
      put(r, "compose.yaml", compose);
      put(r, "Dockerfile", readFileSync(join(r, "Dockerfile"), "utf8").replace("USER 10001\n", ""));
      put(r, ".github/workflows/ci.yml", readFileSync(join(r, ".github/workflows/ci.yml"), "utf8").replace(/checkout@\S+ # v5.0.0/, "checkout@v5"));
      const out = run(r).stdout;
      expect(out).toContain("forge:docker/non-root-user failed\nDockerfile:0 the final stage has no USER");
      if (!out.includes("forge:github-actions/zizmor skipped")) expect(out).toContain("forge:github-actions/zizmor failed\n.github/workflows/ci.yml:13 unpinned-uses");
    },
    T,
  );
});

describe("checks that need no tool", () => {
  /** An applied Next.js repository, committed; the given files are then written and committed too. */
  function nextRepo(files: Record<string, string>): string {
    const r = applied("nextjs-prisma-app");
    commitAll(r, "forge");
    for (const [rel, text] of Object.entries(files)) put(r, rel, text);
    commitAll(r, "change");
    return r;
  }
  const all = (r: string) => sh("sh", [".forge/check.sh", "--all"], { cwd: r });

  /** @rule typescript/no-ignore-deprecations typescript/vitest/no-workspace-file typescript/prisma/client-generator */
  test("the Next.js sample passes them as committed", () => {
    const out = all(nextRepo({})).stdout;
    // the lint stage ran (every check of a stage runs, even after one fails)
    expect(out).toContain("forge:typescript/format-lint");
    expect(out).not.toContain("forge:typescript/no-ignore-deprecations");
    expect(out).not.toContain("forge:typescript/vitest/no-workspace-file");
    expect(out).not.toContain("forge:typescript/prisma/client-generator");
  }, T);

  /** @rule typescript/no-ignore-deprecations */
  test("a tsconfig that sets ignoreDeprecations fails, naming the file and line", () => {
    const r = all(nextRepo({ "tsconfig.base.json": '{\n  "compilerOptions": {\n    "ignoreDeprecations": "6.0"\n  }\n}\n' }));
    expect(r.status).toBe(1);
    expect(r.stdout).toContain("forge:typescript/no-ignore-deprecations failed");
    expect(r.stdout).toContain('tsconfig.base.json:3:    "ignoreDeprecations": "6.0"');
  }, T);

  /** @rule typescript/vitest/no-workspace-file */
  test("a vitest.workspace file fails", () => {
    const r = all(nextRepo({ "vitest.workspace.ts": "export default [];\n" }));
    expect(r.status).toBe(1);
    expect(r.stdout).toContain("forge:typescript/vitest/no-workspace-file failed");
  }, T);

  /** @rule typescript/prisma/client-generator */
  test("a schema with the prisma-client-js generator fails, naming the line", () => {
    const r = all(nextRepo({ "prisma/legacy.prisma": 'generator client {\n  provider = "prisma-client-js"\n}\n' }));
    expect(r.status).toBe(1);
    expect(r.stdout).toContain("forge:typescript/prisma/client-generator failed");
    expect(r.stdout).toContain('prisma/legacy.prisma:2:  provider = "prisma-client-js"');
  }, T);
});

describe("migration linting", () => {
  /** Squawk's advice that does not break a deploy (lock timeouts, IF NOT EXISTS, indexes on new tables) never fails. */
  const PRISMA_CREATE = `-- CreateTable
CREATE TABLE "Post" (
    "id" SERIAL NOT NULL,
    "title" VARCHAR(255) NOT NULL,
    "createdAt" TIMESTAMP(3) NOT NULL DEFAULT CURRENT_TIMESTAMP,
    "authorId" INTEGER NOT NULL,

    CONSTRAINT "Post_pkey" PRIMARY KEY ("id")
);

-- CreateIndex
CREATE INDEX "Post_authorId_idx" ON "Post"("authorId");

-- AddForeignKey
ALTER TABLE "Post" ADD CONSTRAINT "Post_authorId_fkey" FOREIGN KEY ("authorId") REFERENCES "User"("id") ON DELETE RESTRICT ON UPDATE CASCADE;
`;
  const PRISMA_RENAME = 'ALTER TABLE "User" RENAME COLUMN "email" TO "mail";\n';

  /** The Next.js sample, applied and committed, with lock-sync off (the sample's lock file is a stub). */
  function prismaRepo(): string {
    const r = sampleRepo("nextjs-prisma-app");
    put(r, "forge.yaml", 'forge: "0.5"\ndisable:\n  - id: typescript/bun/lock-sync\n    reason: the sample lock file is a stub\n');
    commitAll(r, "config");
    expect(forge(["apply", "--yes"], r).status).toBe(0);
    commitAll(r, "forge");
    return r;
  }
  const run = (r: string, mode: string, env: Record<string, string> = {}) => sh("sh", [".forge/check.sh", mode], { cwd: r, env });

  /** @rule typescript/prisma/migration-lint */
  test.if(hasBun)(
    "a new Prisma migration that renames a column fails, naming the file, line and Squawk rule",
    () => {
      const r = prismaRepo();
      put(r, "prisma/migrations/20261020000000_rename/migration.sql", PRISMA_RENAME);
      const out = run(r, "--changed");
      expect(out.status).toBe(1);
      expect(out.stdout).toContain("forge:typescript/prisma/migration-lint failed");
      expect(out.stdout).toContain("prisma/migrations/20261020000000_rename/migration.sql:1:20: warning: renaming-column");
      expect(out.stdout).toContain("renaming-column");
    },
    T,
  );

  /** @rule typescript/prisma/migration-lint */
  test.if(hasBun)(
    "a typical Prisma migration (new table, index, foreign key) passes although Squawk has other advice for it",
    () => {
      const r = prismaRepo();
      put(r, "prisma/migrations/20261020000000_post/migration.sql", PRISMA_CREATE);
      const out = run(r, "--changed");
      expect(out.stdout).not.toContain("forge:typescript/prisma/migration-lint");
      // 0: every step ran, the migration stage included, and passed
      expect(out.status).toBe(0);
    },
    T,
  );

  /** @rule typescript/prisma/migration-lint */
  test.if(hasBun)(
    "CI lints only the newest migration: a rename merged earlier does not fail every later run",
    () => {
      const r = prismaRepo();
      put(r, "prisma/migrations/20261020000000_rename/migration.sql", PRISMA_RENAME);
      commitAll(r, "rename");
      expect(run(r, "--all").stdout).toContain("forge:typescript/prisma/migration-lint failed");
      put(r, "prisma/migrations/20261021000000_post/migration.sql", PRISMA_CREATE);
      commitAll(r, "post");
      const after = run(r, "--all").stdout;
      expect(after).not.toContain("forge:typescript/prisma/migration-lint");
      expect(after).toContain("forge:typescript/vitest/run skipped");
    },
    T,
  );

  /** @rule typescript/prisma/migration-lint */
  test.if(hasBun)(
    "when Squawk cannot run (offline, no bunx or uvx), the check says so and does not block",
    () => {
      const r = prismaRepo();
      put(r, "prisma/migrations/20261020000000_rename/migration.sql", PRISMA_RENAME);
      const bin = join(r, ".offline-bin");
      for (const name of ["bunx", "uvx"]) {
        put(r, `.offline-bin/${name}`, "#!/bin/sh\necho 'error: could not reach the registry' >&2\nexit 1\n");
        chmodSync(join(bin, name), 0o755);
      }
      const out = run(r, "--changed", { PATH: `${bin}:${process.env.PATH}` });
      expect(out.stdout).toContain("forge:typescript/prisma/migration-lint skipped: squawk could not run (error: could not reach the registry)");
      expect(out.stdout).not.toContain("migration-lint failed");
    },
    T,
  );

  /** The FastAPI sample with a real async Alembic setup (`alembic init -t async`) and the cached Alembic. */
  function alembicRepo(): string {
    ensureTools();
    const r = sampleRepo("fastapi-app");
    const tpl = mkdtempSync(join(tmpdir(), "forge-alembic-"));
    // alembic init writes alembic.ini to its working directory, the scripts to the named one
    expect(sh(join(TOOLS_CACHE, ".venv-alembic/bin/alembic"), ["init", "-t", "async", "alembic"], { cwd: tpl }).status).toBe(0);
    put(r, "alembic/env.py", readFileSync(join(tpl, "alembic/env.py"), "utf8"));
    put(r, "alembic/script.py.mako", readFileSync(join(tpl, "alembic/script.py.mako"), "utf8"));
    put(r, "alembic.ini", readFileSync(join(tpl, "alembic.ini"), "utf8").replace(/^sqlalchemy\.url = .*$/m, "sqlalchemy.url = postgresql+asyncpg://app:app@localhost:5432/app").replace(/^script_location = .*$/m, "script_location = %(here)s/alembic"));
    put(
      r,
      "alembic/versions/0001_users.py",
      '"""users"""\nimport sqlalchemy as sa\nfrom alembic import op\n\nrevision = "0001"\ndown_revision = None\n\n\ndef upgrade() -> None:\n    op.create_table("users", sa.Column("id", sa.Integer, primary_key=True), sa.Column("name", sa.Text))\n\n\ndef downgrade() -> None:\n    op.drop_table("users")\n',
    );
    put(
      r,
      "forge.yaml",
      'forge: "0.5"\ndisable:\n  - id: python/sqlalchemy/alembic/check\n    reason: no database in this test\n  - id: python/uv/lock-sync\n    reason: the sample lock file is a stub\n',
    );
    linkTools(r, "alembic");
    commitAll(r, "alembic");
    expect(forge(["apply", "--yes"], r).status).toBe(0);
    commitAll(r, "forge");
    return r;
  }
  const revision = (body: string, rev = "0002", down = "0001") =>
    `"""next"""\nimport sqlalchemy as sa\nfrom alembic import op\n\nrevision: str = "${rev}"\ndown_revision: str | None = "${down}"\n\n\ndef upgrade() -> None:\n    ${body}\n\n\ndef downgrade() -> None:\n    pass\n`;

  /** @rule python/sqlalchemy/alembic/migration-lint */
  test.if(hasBun && hasUv)(
    "an Alembic revision that renames a column fails: its upgrade is rendered to SQL offline and linted",
    () => {
      const r = alembicRepo();
      put(r, "alembic/versions/0002_rename.py", revision('op.alter_column("users", "name", new_column_name="full_name")'));
      const out = run(r, "--changed");
      expect(out.status).toBe(1);
      expect(out.stdout).toContain("forge:python/sqlalchemy/alembic/migration-lint failed");
      expect(out.stdout).toContain("SQL of alembic upgrade 0001:0002 --sql");
      expect(out.stdout).toContain("renaming-column");
    },
    T,
  );

  /** @rule python/sqlalchemy/alembic/migration-lint */
  test.if(hasBun && hasUv)(
    "an Alembic revision that adds a nullable column passes; CI lints the head revision, which fails when it adds a required column",
    () => {
      const r = alembicRepo();
      put(r, "alembic/versions/0002_nick.py", revision('op.add_column("users", sa.Column("nick", sa.Text, nullable=True))'));
      for (const mode of ["--changed", "--all"]) {
        const out = run(r, mode).stdout;
        expect(out).not.toContain("forge:python/sqlalchemy/alembic/migration-lint");
        // the migration stage passed: the test stage, which comes after it, ran
        expect(out).toContain("forge:python/pytest/run skipped");
        commitAll(r, "nick");
      }
      put(r, "alembic/versions/0003_age.py", revision('op.add_column("users", sa.Column("age", sa.Integer, nullable=False))', "0003", "0002"));
      commitAll(r, "required");
      const all = run(r, "--all");
      expect(all.stdout).toContain("forge:python/sqlalchemy/alembic/migration-lint failed");
      expect(all.stdout).toContain("adding-required-field");
    },
    T,
  );
});

describe("advisory checks block once per turn", () => {
  /** An applied Next.js repository with only the lint stage's tool-free checks left to run, and a turn snapshot at HEAD. */
  function turnRepo(disable: string[]): { repo: string; turn: () => ReturnType<typeof sh> } {
    const r = sampleRepo("nextjs-prisma-app");
    put(r, "forge.yaml", `forge: "0.5"\ndisable:\n${disable.map((id) => `  - {id: ${id}, reason: test}\n`).join("")}`);
    commitAll(r, "forge.yaml");
    expect(forge(["apply", "--yes"], r).status).toBe(0);
    commitAll(r, "forge");
    const snap = ".forge/state/t.turn";
    put(r, `${snap}/head`, git(r, "rev-parse", "HEAD").stdout);
    put(r, `${snap}/tree`, "\n");
    put(r, `${snap}/dirty`, "");
    return { repo: r, turn: () => sh("sh", [".forge/check.sh", "--turn", snap], { cwd: r }) };
  }

  /** @rule typescript/lockfile-updated */
  test(
    "a dependency added to package.json without a lock-file change blocks the first Stop of the turn and is accepted at the next; pre-commit always fails",
    () => {
      const { repo: r, turn } = turnRepo(["typescript/bun/lock-sync", "typescript/bun/release-age", "typescript/bun/trusted-deps", "typescript/config-ratchet"]);
      const pkg = readFileSync(join(r, "package.json"), "utf8");
      put(r, "package.json", pkg.replace('"zod": "4.1.12"', '"zod": "4.1.12", "esbuild": "0.25.5"'));
      const first = turn();
      expect(first.status).toBe(1);
      expect(first.stdout).toContain("forge:typescript/lockfile-updated failed");
      expect(first.stdout).toContain("package.json: dependencies esbuild 0.25.5 added, but bun.lock did not change");
      const second = turn();
      expect(second.stdout).toContain("forge:typescript/lockfile-updated accepted: reported once this turn");
      expect(second.stdout).not.toContain("forge:typescript/lockfile-updated failed");
      expect(sh("sh", [".forge/check.sh", "--changed"], { cwd: r }).stdout).toContain("forge:typescript/lockfile-updated failed");
    },
    T,
  );
  /** @rule typescript/vitest/snapshot-stale */
  test(
    "a change to output that a snapshot test imports, without a snapshot change, blocks the first Stop of the turn and is accepted at the next",
    () => {
      const { repo: r, turn } = turnRepo(["typescript/bun/lock-sync", "typescript/config-ratchet"]);
      put(r, "src/span.ts", "export const span = (name: string) => ({ name });\n");
      put(r, "src/span.test.ts", 'import { expect, it } from "vitest";\nimport { span } from "./span.js";\n\nit("span", () => {\n  expect(span("a")).toMatchSnapshot();\n});\n');
      put(r, "src/__snapshots__/span.test.ts.snap", "exports[`span 1`] = `{}`;\n");
      commitAll(r, "span");
      put(r, ".forge/state/t.turn/head", git(r, "rev-parse", "HEAD").stdout);
      put(r, "src/span.ts", 'export const span = (name: string) => ({ name, kind: "llm" });\n');
      const first = turn();
      expect(first.status).toBe(1);
      expect(first.stdout).toContain("forge:typescript/vitest/snapshot-stale failed");
      expect(first.stdout).toContain("src/span.ts: imported by src/span.test.ts, which compares output with snapshots or golden files");
      expect(turn().stdout).toContain("forge:typescript/vitest/snapshot-stale accepted: reported once this turn");
      // pre-commit and CI do not run it: a human updates snapshots on purpose
      expect(sh("sh", [".forge/check.sh", "--changed"], { cwd: r }).stdout).not.toContain("forge:typescript/vitest/snapshot-stale");
    },
    T,
  );
});

describe("project tools whose name is not a shell function name", () => {
  const rule = loadPacks(join(ROOT, "packs")).get("typescript/vue")!.rules.find((r) => r.id === "typescript/vue/typecheck")!;
  const target = rule.enforce[0] as CheckTarget;
  const example = rule.examples!.match![0]!;

  /**
   * POSIX sh (dash) rejects "-" in a function name, so vue-tsc gets no wrapper
   * function: the check runs it as `forge_exec_tool vue-tsc`, which still
   * resolves the project's own binary and reports it missing.
   * @rule typescript/vue/typecheck
   */
  test("vue-tsc is run through forge_exec_tool, is a tool of the check, and is reported skipped when the project does not have it", () => {
    expect(checkTools(target)).toEqual(["vue-tsc"]);
    expect(renderChecks([{ rule, target }])).not.toContain("vue-tsc()");
    expect(runCheckExample(rule, target, example, "match", [])).toEqual({ outcome: "skip", detail: "vue-tsc not available" });
    const bin = mkdtempSync(join(tmpdir(), "forge-vue-tsc-"));
    writeFileSync(join(bin, "vue-tsc"), "#!/bin/sh\nprintf 'src/Counter.vue(2,7): error TS2322: Type string is not assignable to type number.\\n'\nexit 2\n");
    chmodSync(join(bin, "vue-tsc"), 0o755);
    expect(runCheckExample(rule, target, example, "match", [bin]).outcome).toBe("pass");
  });
});
