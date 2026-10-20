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
import { chmodSync, existsSync, mkdirSync, readFileSync, symlinkSync, unlinkSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { applied, commitAll, forge, git, put, sampleRepo, sh } from "../helpers";

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
});

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

  test.each([[".husky"], ["lefthook.yml"], [".pre-commit-config.yaml"]])(
    "with %s present no hook is created and status prints the line to add",
    (manager) => {
      const r = sampleRepo("eslint-app");
      if (manager === ".husky") mkdirSync(join(r, ".husky"));
      else put(r, manager, "# hooks\n");
      expect(forge(["apply", "--yes"], r).status).toBe(0);
      expect(existsSync(join(r, ".git/hooks/pre-commit"))).toBe(false);
      expect(forge(["status"], r).stdout).toContain(`add "sh .forge/check.sh --changed" to ${manager}`);
    },
    T,
  );

  test(
    "with core.hooksPath set no hook is created",
    () => {
      const r = sampleRepo("eslint-app");
      git(r, "config", "core.hooksPath", "githooks");
      expect(forge(["apply", "--yes"], r).status).toBe(0);
      expect(existsSync(join(r, ".git/hooks/pre-commit"))).toBe(false);
      expect(forge(["status"], r).stdout).toContain("core.hooksPath (githooks)");
    },
    T,
  );

  test(
    "an existing pre-commit hook is never touched, by apply or by remove",
    () => {
      const r = sampleRepo("eslint-app");
      put(r, ".git/hooks/pre-commit", "#!/bin/sh\necho mine\n");
      expect(forge(["apply", "--yes"], r).status).toBe(0);
      expect(forge(["remove", "--yes"], r).status).toBe(0);
      expect(readFileSync(join(r, ".git/hooks/pre-commit"), "utf8")).toBe("#!/bin/sh\necho mine\n");
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
