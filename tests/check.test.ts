// check.sh and the project hooks on the fastapi sample (ast-grep through bunx/uvx).
import { beforeAll, describe, expect, test } from "bun:test";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, rmSync, symlinkSync, unlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { forge, git, hookJson, sampleRepo, sh } from "./helpers";

const T = 180_000;
const SCRIPTS = ".claude/skills/forge-project/scripts";
let repo = "";

function check(mode = "--changed", env: Record<string, string> = {}) {
  return sh("sh", [".forge/check.sh", mode], { cwd: repo, env });
}
function hook(script: string, fields: Record<string, unknown>, env: Record<string, string> = {}) {
  return sh("sh", [join(SCRIPTS, script)], { cwd: repo, input: hookJson({ cwd: repo, ...fields }), env });
}
function write(rel: string, text: string) {
  mkdirSync(join(repo, rel, ".."), { recursive: true });
  writeFileSync(join(repo, rel), text);
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
  repo = sampleRepo("fastapi-app");
  expect(forge(["apply", "--yes"], repo).status).toBe(0);
  git(repo, "add", "-A");
  git(repo, "commit", "-qm", "forge");
  mkdirSync(join(repo, ".venv", "bin"), { recursive: true });
  const ruff = sh("sh", ["-c", "command -v ruff"]).stdout.trim();
  if (ruff) symlinkSync(ruff, join(repo, ".venv", "bin", "ruff"));
}, T);

describe("check.sh --changed", () => {
  test("clean tree passes", () => expect(check().status).toBe(0), T);

  test(
    "forbid rule",
    () => {
      write("app/repo.py", "def f(session):\n    return session.query(1)\n");
      const r = check();
      expect(r.status).toBe(1);
      expect(r.stdout).toContain("forge:sqlalchemy/no-query-api app/repo.py:2:12");
      reset();
    },
    T,
  );

  test(
    "applied migration modified",
    () => {
      write("alembic/versions/0001_users.py", '"""users"""\n\nrevision = "0001"\ndown_revision = "x"\n');
      const r = check();
      expect(r.status).toBe(1);
      expect(r.stdout).toContain("forge:forge/applied-migration alembic/versions/0001_users.py");
      reset();
    },
    T,
  );

  test(
    "deleted test file",
    () => {
      unlinkSync(join(repo, "tests/test_main.py"));
      const r = check();
      expect(r.stdout).toContain("forge:forge/deleted-test tests/test_main.py was deleted");
      reset();
    },
    T,
  );

  test(
    "removed assert (no-decrease) and new skip (no-increase, file new since HEAD)",
    () => {
      write("tests/test_main.py", "from app.main import health\n\n\nasync def test_health() -> None:\n    await health()\n");
      write("tests/test_new.py", 'import pytest\n\n\n@pytest.mark.skip(reason="x")\ndef test_x() -> None:\n    assert True\n');
      const r = check();
      expect(r.status).toBe(1);
      expect(r.stdout).toContain("forge:pytest/no-fewer-asserts tests/test_main.py: 1 -> 0");
      expect(r.stdout).toContain("forge:pytest/no-new-skip tests/test_new.py: 0 -> 1");
      reset();
    },
    T,
  );

  test(
    "remote DATABASE_URL: migration checks skipped; local: they run",
    () => {
      stub("alembic", 'echo "FAILED: Target database is not up to date."; exit 1');
      write("app/models/__init__.py", "from sqlalchemy.orm import DeclarativeBase\n\n\nclass Base(DeclarativeBase):\n    pass\n");
      const remote = check("--changed", { DATABASE_URL: "postgresql://u:p@db.prod.example.com/app" });
      expect(remote.stdout).toContain("forge:alembic/check skipped: no local database");
      const local = check("--changed", { DATABASE_URL: "postgresql://u:p@localhost/app" });
      expect(local.status).toBe(1);
      expect(local.stdout).toContain("forge:alembic/check failed");
      unlinkSync(join(repo, ".venv/bin/alembic"));
      const absent = check();
      expect(absent.stdout).toContain("forge:alembic/check skipped: alembic not installed");
      reset();
    },
    T,
  );

  test(
    "pytest changed_run gets the related test files",
    () => {
      stub("pytest", 'printf "ARGS:%s\\n" "$*"; exit 1');
      write("app/main.py", 'from fastapi import FastAPI\n\napp = FastAPI()\n\n\nasync def health() -> dict[str, str]:\n    return {"status": "ok"}\n');
      write("tests/test_main.py", "from app.main import health\n\n\nasync def test_health() -> None:\n    assert await health() == {\"status\": \"ok\"}\n    assert True\n");
      const r = check();
      expect(r.stdout).toContain("forge:pytest/run failed");
      expect(r.stdout).toContain("ARGS:-q tests/test_main.py");
      unlinkSync(join(repo, ".venv/bin/pytest"));
      reset();
    },
    T,
  );
});

describe("hooks", () => {
  test(
    "post-edit: lint and ast-grep findings as additionalContext; nothing on success",
    () => {
      write("app/q.py", "import os\n\ndef f(session):\n    return session.query(1)\n");
      const r = hook("post-edit.sh", { tool_name: "Write", tool_input: { file_path: join(repo, "app/q.py") } });
      expect(r.status).toBe(0);
      const out = JSON.parse(r.stdout).hookSpecificOutput;
      expect(out.hookEventName).toBe("PostToolUse");
      expect(out.additionalContext).toContain("forge:sqlalchemy/no-query-api app/q.py");
      if (existsSync(join(repo, ".venv/bin/ruff"))) expect(out.additionalContext).toContain("F401");
      expect(out.additionalContext.split("\n").length).toBeLessThanOrEqual(20);
      write("app/ok.py", "X = 1\n");
      const ok = hook("post-edit.sh", { tool_name: "Write", tool_input: { file_path: join(repo, "app/ok.py") } });
      expect(ok.stdout).toBe("");
      reset();
    },
    T,
  );

  test(
    "stop: blocks 3 times, then hands over; a pass is remembered",
    () => {
      write("app/q.py", "def f(session):\n    return session.query(1)\n");
      for (let i = 0; i < 3; i++) {
        const r = hook("stop.sh", { session_id: "s-1" });
        expect(JSON.parse(r.stdout).decision).toBe("block");
      }
      expect(JSON.parse(hook("stop.sh", { session_id: "s-1" }).stdout).systemMessage).toContain("Handing over to you");
      reset();
      expect(hook("stop.sh", { session_id: "s-1" }).stdout).toBe("");
      expect(existsSync(join(repo, ".forge/state/last-pass"))).toBe(true);
    },
    T,
  );

  test(
    "stop: editing check.sh through Bash fails on integrity",
    () => {
      sh("sh", ["-c", "sed 's/^fail=0$/fail=0 # x/' .forge/check.sh > .forge/c && cat .forge/c > .forge/check.sh && rm .forge/c"], { cwd: repo });
      const r = hook("stop.sh", { session_id: "s-2" });
      expect(JSON.parse(r.stdout).reason).toContain("forge:forge/integrity .forge/check.sh was modified");
      reset();
    },
    T,
  );

  test(
    "typecheck: errors exit 2 on stderr; concurrent run skipped",
    () => {
      stub("basedpyright", 'sleep 1; echo "app/main.py:1:1 - error: nope"; exit 1');
      const input = hookJson({ cwd: repo, tool_name: "Edit", tool_input: { file_path: join(repo, "app/main.py") } });
      const r = sh("sh", ["-c", `sh ${SCRIPTS}/typecheck.sh <"$1" & sleep 0.3; sh ${SCRIPTS}/typecheck.sh <"$1"; echo "second=$?"; wait $!; echo "first=$?"`, "x", writeInput(input)], {
        cwd: repo,
      });
      expect(r.stdout).toContain("second=0");
      expect(r.stdout).toContain("first=2");
      expect(r.stderr).toContain("forge:python/typecheck");
      unlinkSync(join(repo, ".venv/bin/basedpyright"));
      expect(existsSync(join(repo, ".forge/state/typecheck.lock"))).toBe(false);
    },
    T,
  );

  test(
    "session-start: <= 20 lines; missing tools reported once",
    () => {
      for (let i = 0; i < 15; i++) write(`notes/n${i}.txt`, "x\n");
      rmSync(join(repo, ".forge/state/missing-tools"), { force: true });
      const first = hook("session-start.sh", { source: "startup" });
      expect(first.status).toBe(0);
      const lines = first.stdout.trimEnd().split("\n");
      expect(lines.length).toBeLessThanOrEqual(20);
      expect(lines[0]).toBe("forge: branch main, 15 changed files");
      expect(first.stdout).toContain("forge: packs alembic, fastapi, pydantic, pytest, python, sqlalchemy, uv");
      expect(first.stdout).toContain("forge: missing tools");
      const again = hook("session-start.sh", { source: "compact" });
      expect(again.stdout).toContain("forge: branch main");
      expect(again.stdout).not.toContain("missing tools");
      reset();
    },
    T,
  );
});

function writeInput(text: string): string {
  const f = join(mkdtempSync(join(tmpdir(), "forge-in-")), "in.json");
  writeFileSync(f, text);
  return f;
}

test(
  "TypeScript repo without bun on PATH: hooks exit 0, SessionStart reports bun",
  () => {
    const ts = sampleRepo("nextjs-prisma-app");
    expect(forge(["apply", "--yes"], ts).status).toBe(0);
    const env = { PATH: "/usr/bin:/bin" };
    const run = (script: string, fields: Record<string, unknown>) =>
      sh("sh", [join(SCRIPTS, script)], { cwd: ts, input: hookJson({ cwd: ts, ...fields }), env });
    const edit = { tool_name: "Edit", tool_input: { file_path: join(ts, "src/sum.ts") } };
    const ss = run("session-start.sh", {});
    expect(ss.status).toBe(0);
    expect(ss.stdout).toMatch(/missing tools.*\bbun\b/);
    expect(run("session-start.sh", {}).stdout).not.toContain("missing tools");
    expect(run("post-edit.sh", edit).status).toBe(0);
    expect(run("typecheck.sh", edit).status).toBe(0);
    expect(run("guard.sh", { tool_name: "Bash", tool_input: { command: "ls" } }).status).toBe(0);
  },
  T,
);
