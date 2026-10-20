/**
 * @spec 8 · Project hooks
 *
 * At project level Forge writes a `forge-project` plugin whose hooks run with
 * plain `sh`. They read paths and the working directory from the hook input,
 * never from `CLAUDE_PROJECT_DIR`, and print nothing on success. These tests
 * run the generated scripts on the sample FastAPI repository with real hook
 * JSON; ast-grep runs through `bunx` or `uvx`.
 */
import { beforeAll, describe, expect, test } from "bun:test";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { applied, commitAll, git, hookJson, put, sh } from "../helpers";

const T = 180_000;
const SCRIPTS = ".claude/skills/forge-project/scripts";
let repo = "";

function hook(script: string, fields: Record<string, unknown>, env: Record<string, string> = {}, cwd = repo) {
  return sh("sh", [join(SCRIPTS, script)], { cwd, input: hookJson({ cwd, ...fields }), env });
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
const edit = (rel: string) => ({ tool_name: "Edit", tool_input: { file_path: join(repo, rel) } });

beforeAll(() => {
  repo = applied("fastapi-app");
  commitAll(repo, "forge");
  mkdirSync(join(repo, ".venv", "bin"), { recursive: true });
  const ruff = sh("sh", ["-c", "command -v ruff"]).stdout.trim();
  if (ruff) symlinkSync(ruff, join(repo, ".venv", "bin", "ruff"));
}, T);

test(
  "hooks.json wires SessionStart, PreToolUse(Bash), PostToolUse(Edit|Write) twice and Stop, with the specified timeouts",
  () => {
    const hooks = JSON.parse(readFileSync(join(repo, ".claude/skills/forge-project/hooks/hooks.json"), "utf8")).hooks;
    expect(hooks.SessionStart[0]).toMatchObject({ matcher: "startup|resume|clear|compact", hooks: [{ timeout: 10 }] });
    expect(hooks.PreToolUse[0]).toMatchObject({ matcher: "Bash", hooks: [{ timeout: 5, onFailure: "block" }] });
    expect(hooks.PostToolUse[0].matcher).toBe("Edit|Write");
    expect(hooks.PostToolUse[0].hooks).toMatchObject([{ timeout: 30 }, { timeout: 300, asyncRewake: true }]);
    expect(hooks.Stop[0].hooks).toMatchObject([{ timeout: 600 }]);
  },
  T,
);

describe("SessionStart", () => {
  test(
    "session-start prints at most 20 lines: branch, changed files (first 10), last commits, active packs",
    () => {
      for (let i = 0; i < 15; i++) put(repo, `notes/n${i}.txt`, "x\n");
      const out = hook("session-start.sh", { source: "startup" }).stdout;
      const lines = out.trimEnd().split("\n");
      expect(lines.length).toBeLessThanOrEqual(20);
      expect(lines[0]).toBe("forge: branch main, 15 changed files");
      expect(lines.filter((l) => l.startsWith("  notes/")).length).toBe(10);
      expect(out).toContain("forge: last commits\n  ");
      expect(out).toContain("forge: packs python, python/fastapi, python/pydantic, python/pytest, python/sqlalchemy, python/sqlalchemy/alembic, python/uv");
      reset();
    },
    T,
  );

  test(
    "missing tools are reported once, not at every session start",
    () => {
      rmSync(join(repo, ".forge/state/missing-tools"), { force: true });
      expect(hook("session-start.sh", { source: "startup" }).stdout).toContain("forge: missing tools");
      expect(hook("session-start.sh", { source: "compact" }).stdout).not.toContain("missing tools");
    },
    T,
  );
});

describe("PostToolUse", () => {
  test(
    "post-edit formats the file silently, then returns lint and ast-grep findings for this file as additionalContext (≤ 20 lines)",
    () => {
      put(repo, "app/q.py", "import os\n\ndef f(session):\n    return session.query(1)\n");
      const r = hook("post-edit.sh", edit("app/q.py"));
      expect(r.status).toBe(0);
      const out = JSON.parse(r.stdout).hookSpecificOutput;
      expect(out.hookEventName).toBe("PostToolUse");
      expect(out.additionalContext).toContain("forge:python/sqlalchemy/no-query-api app/q.py:5:12");
      if (!existsSync(join(repo, ".venv/bin/ruff"))) throw new Error("ruff is required on PATH for this test");
      expect(out.additionalContext).toContain("F401");
      expect(readFileSync(join(repo, "app/q.py"), "utf8")).toBe("import os\n\n\ndef f(session):\n    return session.query(1)\n");
      expect(out.additionalContext.split("\n").length).toBeLessThanOrEqual(20);
      reset();
    },
    T,
  );

  test(
    "post-edit prints nothing when the file is clean",
    () => {
      put(repo, "app/ok.py", "X = 1\n");
      expect(hook("post-edit.sh", edit("app/ok.py")).stdout).toBe("");
      reset();
    },
    T,
  );

  test(
    "typecheck reports errors with exit 2 on stderr, and skips while another run holds the lock",
    () => {
      stub("basedpyright", 'sleep 1; echo "app/main.py:1:1 - error: nope"; exit 1');
      const input = join(mkdtempSync(join(tmpdir(), "forge-in-")), "in.json");
      writeFileSync(input, hookJson({ cwd: repo, ...edit("app/main.py") }));
      const r = sh("sh", ["-c", `sh ${SCRIPTS}/typecheck.sh <"$1" & sleep 0.3; sh ${SCRIPTS}/typecheck.sh <"$1"; echo "second=$?"; wait $!; echo "first=$?"`, "x", input], { cwd: repo });
      expect(r.stdout).toContain("second=0");
      expect(r.stdout).toContain("first=2");
      expect(r.stderr).toContain("forge:python/typecheck");
      expect(existsSync(join(repo, ".forge/state/typecheck.lock"))).toBe(false);
      rmSync(join(repo, ".venv/bin/basedpyright"));
    },
    T,
  );

  test(
    "typecheck takes over a lock whose process is gone",
    () => {
      stub("basedpyright", "exit 0");
      mkdirSync(join(repo, ".forge/state/typecheck.lock"), { recursive: true });
      writeFileSync(join(repo, ".forge/state/typecheck.lock/pid"), "999999\n");
      expect(hook("typecheck.sh", edit("app/main.py")).status).toBe(0);
      expect(existsSync(join(repo, ".forge/state/typecheck.lock"))).toBe(false);
      rmSync(join(repo, ".venv/bin/basedpyright"));
    },
    T,
  );
});

describe("Stop", () => {
  test(
    "stop blocks with the check output while check.sh fails, then hands over after 3 blocks",
    () => {
      put(repo, "app/q.py", "def f(session):\n    return session.query(1)\n");
      for (let i = 0; i < 3; i++) {
        const out = JSON.parse(hook("stop.sh", { session_id: "s-1" }).stdout);
        expect(out.decision).toBe("block");
        expect(out.reason).toContain("forge:python/sqlalchemy/no-query-api app/q.py:2:12");
      }
      expect(JSON.parse(hook("stop.sh", { session_id: "s-1" }).stdout)).toEqual({ systemMessage: "forge: check failed 3 times. Handing over to you." });
      reset();
    },
    T,
  );

  test(
    "stop remembers a passing tree and does not run the check again until it changes",
    () => {
      expect(hook("stop.sh", { session_id: "s-2" }).stdout).toBe("");
      expect(existsSync(join(repo, ".forge/state/last-pass"))).toBe(true);
      const t0 = performance.now();
      expect(hook("stop.sh", { session_id: "s-2" }).stdout).toBe("");
      expect(performance.now() - t0).toBeLessThan(1000);
    },
    T,
  );

  test(
    "editing .forge/check.sh through Bash fails the next Stop on integrity",
    () => {
      sh("sh", ["-c", "sed 's/^fail=0$/fail=0 # x/' .forge/check.sh > .forge/c && cat .forge/c > .forge/check.sh && rm .forge/c"], { cwd: repo });
      expect(JSON.parse(hook("stop.sh", { session_id: "s-3" }).stdout).reason).toContain("forge:forge/integrity .forge/check.sh was modified");
      reset();
    },
    T,
  );
});

test(
  "without bun on PATH in a TypeScript repository every hook exits 0 and SessionStart reports bun once",
  () => {
    const ts = applied("nextjs-prisma-app");
    const env = { PATH: "/usr/bin:/bin" };
    const run = (script: string, fields: Record<string, unknown>) => hook(script, fields, env, ts);
    const tsEdit = { tool_name: "Edit", tool_input: { file_path: join(ts, "src/sum.ts") } };
    const first = run("session-start.sh", {});
    expect(first.status).toBe(0);
    expect(first.stdout).toMatch(/missing tools.*\bbun\b/);
    expect(run("session-start.sh", {}).stdout).not.toContain("missing tools");
    expect(run("post-edit.sh", tsEdit).status).toBe(0);
    expect(run("typecheck.sh", tsEdit).status).toBe(0);
    expect(run("guard.sh", { tool_name: "Bash", tool_input: { command: "ls" } }).status).toBe(0);
  },
  T,
);
