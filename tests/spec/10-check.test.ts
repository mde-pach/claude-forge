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
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, symlinkSync, unlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { applied, commitAll, ensureTools, forge, git, hasBun, hasUv, linkTools, put, sampleRepo, sh, TOOLS_CACHE } from "../helpers";

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
      expect(ok.stdout).toContain("forge:local/lint-all ok for the changed files");
      expect(ok.stdout).not.toContain("forge:local/lint-all failed");
      put(r, "app/new.py", "import sys\n");
      const bad = sh("sh", [".forge/check.sh", "--changed"], { cwd: r });
      expect(bad.stdout).toContain("forge:local/lint-all failed\napp/new.py:1:8: F401");
      expect(bad.stdout).not.toContain("app/legacy.py");
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
