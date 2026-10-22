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
import { applied, commitAll, ensureTools, forge, git, hasBun, hookJson, linkTools, put, sampleRepo, sh } from "../helpers";

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
  "hooks.json wires SessionStart, UserPromptSubmit, PreToolUse(Bash), PermissionRequest, PostToolUse(Edit|Write) twice and Stop, with the specified timeouts",
  () => {
    const hooks = JSON.parse(readFileSync(join(repo, ".claude/skills/forge-project/hooks/hooks.json"), "utf8")).hooks;
    expect(hooks.SessionStart[0]).toMatchObject({ matcher: "startup|resume|clear|compact", hooks: [{ timeout: 10 }] });
    expect(hooks.UserPromptSubmit[0]).toMatchObject({ hooks: [{ command: 'sh "${CLAUDE_PLUGIN_ROOT}/scripts/turn-start.sh"', timeout: 10 }] });
    expect(hooks.PreToolUse[0]).toMatchObject({ matcher: "Bash", hooks: [{ timeout: 180, onFailure: "block" }] });
    expect(hooks.PermissionRequest[0]).toMatchObject({ hooks: [{ command: 'sh "${CLAUDE_PLUGIN_ROOT}/scripts/review.sh"', timeout: 180 }] });
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

/**
 * In an autonomous run (claude -p, the Agent SDK, the GitHub Action) nobody can
 * answer an ask, and Claude Code denies it unseen. Forge sends it to an
 * independent reviewer instead: a fresh `claude -p` that sees only the action.
 * These tests put a stand-in `claude` on PATH that records how it was called.
 */
describe("asks nobody can answer", () => {
  let ts = "";
  let bin = "";
  beforeAll(() => {
    ts = applied("nextjs-prisma-app");
    bin = mkdtempSync(join(tmpdir(), "forge-fake-claude-"));
    writeFileSync(
      join(bin, "claude"),
      '#!/bin/sh\n{ printf "FORGE_REVIEWER=%s\\n" "$FORGE_REVIEWER"; for a in "$@"; do printf "ARG:%s\\n" "$a"; done; } > "$FAKE_LOG"\nprintf \'{"type":"result","result":"{}","structured_output":{"reason":"%s","decision":"%s"}}\\n\' "$FAKE_REASON" "$FAKE_DECISION"\n',
    );
    chmodSync(join(bin, "claude"), 0o755);
  }, T);
  const log = () => join(bin, "log");
  function run(script: string, fields: Record<string, unknown>, env: Record<string, string>) {
    rmSync(log(), { force: true });
    return sh("sh", [join(SCRIPTS, script)], {
      cwd: ts,
      input: hookJson({ cwd: ts, permission_mode: "default", ...fields }),
      env: { PATH: `${bin}:${process.env.PATH}`, FAKE_LOG: log(), FAKE_REASON: "it is routine", FAKE_DECISION: "allow", CLAUDE_CODE_ENTRYPOINT: "sdk-cli", FORGE_ASK_REVIEW: "", FORGE_REVIEWER: "", ...env },
    });
  }
  const npm = { hook_event_name: "PreToolUse", tool_name: "Bash", tool_input: { command: "npm install left-pad" } };
  const decision = (out: string) => JSON.parse(out).hookSpecificOutput;

  /** @rule typescript/bun/no-other-pm */
  test(
    "in a headless run (CLAUDE_CODE_ENTRYPOINT sdk-cli, sdk-ts, sdk-py, claude-code-github-action) a guard ask is reviewed: allow or deny with the reviewer's reason",
    () => {
      const ok = decision(run("guard.sh", npm, {}).stdout);
      expect(ok.permissionDecision).toBe("allow");
      expect(ok.permissionDecisionReason).toBe("forge:typescript/bun/no-other-pm reviewed by an independent session, nobody being there to answer: it is routine");
      const no = decision(run("guard.sh", npm, { FAKE_DECISION: "deny", FAKE_REASON: "Bun project", CLAUDE_CODE_ENTRYPOINT: "sdk-ts" }).stdout);
      expect(no.permissionDecision).toBe("deny");
      expect(no.permissionDecisionReason).toContain("nobody being there to answer: Bun project");
      expect(readFileSync(join(ts, ".forge/state/reviews.log"), "utf8")).toContain("\tdeny\tBash\t");
    },
    T,
  );

  test(
    "a PermissionRequest is answered with the reviewer's decision (behavior allow, or deny with a message)",
    () => {
      const edit = { hook_event_name: "PermissionRequest", tool_name: "Edit", tool_input: { file_path: join(ts, "biome.json"), old_string: "a", new_string: "b" } };
      expect(decision(run("review.sh", edit, {}).stdout).decision).toEqual({ behavior: "allow" });
      expect(decision(run("review.sh", edit, { FAKE_DECISION: "deny", FAKE_REASON: "weakens lint" }).stdout).decision).toEqual({
        behavior: "deny",
        message: "forge: denied by an independent review, nobody being there to answer: weakens lint",
      });
    },
    T,
  );

  test(
    "the reviewer sees only the action: a fresh claude -p with read-only tools, no hooks, settings or MCP servers, and not the agent's transcript",
    () => {
      run("guard.sh", { ...npm, transcript_path: "/secret/transcript.jsonl" }, {});
      const call = readFileSync(log(), "utf8");
      expect(call).toContain("FORGE_REVIEWER=1\nARG:-p\n");
      for (const arg of ["--tools\nARG:Read,Grep,Glob", "--permission-mode\nARG:dontAsk", "--settings\nARG:{\"disableAllHooks\":true}", "--setting-sources\nARG:\n", "--strict-mcp-config", "--no-session-persistence", "--json-schema"])
        expect(call).toContain(arg);
      expect(call).toContain('Input: {"command":"npm install left-pad"}');
      expect(call).toContain("Forge flagged it with rule typescript/bun/no-other-pm.");
      expect(call).not.toContain("/secret/transcript.jsonl");
    },
    T,
  );

  test.each([
    ["an interactive session (entrypoint cli): the usual prompt", { CLAUDE_CODE_ENTRYPOINT: "cli" }, {}],
    ["FORGE_ASK_REVIEW=never", { FORGE_ASK_REVIEW: "never" }, {}],
    ["dontAsk mode (the user chose to deny asks)", {}, { permission_mode: "dontAsk" }],
    ["inside a review (FORGE_REVIEWER set)", { FORGE_REVIEWER: "1" }, {}],
  ])("no review for %s: the guard keeps its ask", (_label, env, fields) => {
    expect(decision(run("guard.sh", { ...npm, ...fields }, env).stdout).permissionDecision).toBe("ask");
    expect(existsSync(log())).toBe(false);
  }, T);

  test(
    "FORGE_ASK_REVIEW=always reviews even when a human could answer",
    () => {
      expect(decision(run("guard.sh", npm, { CLAUDE_CODE_ENTRYPOINT: "cli", FORGE_ASK_REVIEW: "always" }).stdout).permissionDecision).toBe("allow");
    },
    T,
  );

  test(
    "without claude on PATH, or without a verdict, nothing changes: the ask stays, and a PermissionRequest gets no answer",
    () => {
      expect(decision(run("guard.sh", npm, { PATH: "/usr/bin:/bin" }).stdout).permissionDecision).toBe("ask");
      expect(run("review.sh", { hook_event_name: "PermissionRequest", tool_name: "Edit", tool_input: {} }, { FAKE_DECISION: "maybe" }).stdout).toBe("");
    },
    T,
  );

  test(
    "the project-level PermissionRequest hook defers to the user-level one, so a request is reviewed once",
    () => {
      const home = mkdtempSync(join(tmpdir(), "forge-home-"));
      mkdirSync(join(home, ".claude/skills/forge-user/scripts"), { recursive: true });
      writeFileSync(join(home, ".claude/skills/forge-user/scripts/review.sh"), "");
      expect(run("review.sh", { hook_event_name: "PermissionRequest", tool_name: "Edit", tool_input: {} }, { HOME: home }).stdout).toBe("");
      expect(existsSync(log())).toBe(false);
    },
    T,
  );
});

describe("PostToolUse", () => {
  /** @rule python/format python/lint python/sqlalchemy/no-query-api */
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

  /** @rule python/typecheck */
  test(
    "typecheck reports errors with exit 2 on stderr, and skips while another run holds the lock",
    () => {
      stub("basedpyright", 'sleep 1; echo "app/main.py:1:1 - error: nope"; exit 1');
      put(repo, "app/main.py", readFileSync(join(repo, "app/main.py"), "utf8") + "\n# edited\n");
      const input = join(mkdtempSync(join(tmpdir(), "forge-in-")), "in.json");
      writeFileSync(input, hookJson({ cwd: repo, ...edit("app/main.py") }));
      const r = sh("sh", ["-c", `sh ${SCRIPTS}/typecheck.sh <"$1" & sleep 0.3; sh ${SCRIPTS}/typecheck.sh <"$1"; echo "second=$?"; wait $!; echo "first=$?"`, "x", input], { cwd: repo });
      expect(r.stdout).toContain("second=0");
      expect(r.stdout).toContain("first=2");
      expect(r.stderr).toContain("forge:python/typecheck");
      expect(existsSync(join(repo, ".forge/state/typecheck.lock"))).toBe(false);
      rmSync(join(repo, ".venv/bin/basedpyright"));
      reset();
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
      reset();
    },
    T,
  );
});

describe("Stop", () => {
  /** @rule python/sqlalchemy/no-query-api */
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
      // within a turn, the pass is remembered in the turn's snapshot
      hook("turn-start.sh", { session_id: "s-2" });
      put(repo, "app/ok.py", "X = 1\n");
      expect(hook("stop.sh", { session_id: "s-2" }).stdout).toBe("");
      expect(existsSync(join(repo, ".forge/state/s-2.turn/pass"))).toBe(true);
      reset();
    },
    T,
  );

  test(
    "work the user left uncommitted before the turn does not block Claude when the turn changes nothing",
    () => {
      put(repo, "app/wip.py", "def f(session):\n    return session.query(1)\n");
      expect(hook("turn-start.sh", { session_id: "t-1", hook_event_name: "UserPromptSubmit", prompt: "What does app/main.py do?" }).stdout).toBe("");
      expect(hook("stop.sh", { session_id: "t-1" }).stdout).toBe("");
      reset();
    },
    T,
  );

  test(
    "a change made during the turn runs the check",
    () => {
      hook("turn-start.sh", { session_id: "t-2" });
      put(repo, "app/q.py", "def f(session):\n    return session.query(1)\n");
      const out = JSON.parse(hook("stop.sh", { session_id: "t-2" }).stdout);
      expect(out.decision).toBe("block");
      expect(out.reason).toContain("forge:python/sqlalchemy/no-query-api app/q.py:2:12");
      reset();
    },
    T,
  );

  test(
    "without a turn-start snapshot (e.g. the first Stop after forge apply) the check runs on every changed file, as before",
    () => {
      rmSync(join(repo, ".forge/state/t-3.turn"), { recursive: true, force: true });
      put(repo, "app/wip.py", "def f(session):\n    return session.query(1)\n");
      expect(JSON.parse(hook("stop.sh", { session_id: "t-3" }).stdout).decision).toBe("block");
      reset();
    },
    T,
  );

  test(
    "a new prompt resets the Stop block counter",
    () => {
      put(repo, "app/q.py", "def f(session):\n    return session.query(1)\n");
      for (let i = 0; i < 2; i++) expect(JSON.parse(hook("stop.sh", { session_id: "t-4" }).stdout).decision).toBe("block");
      hook("turn-start.sh", { session_id: "t-4" });
      put(repo, "app/q2.py", "def g(session):\n    return session.query(2)\n");
      for (let i = 0; i < 3; i++) expect(JSON.parse(hook("stop.sh", { session_id: "t-4" }).stdout).decision).toBe("block");
      expect(JSON.parse(hook("stop.sh", { session_id: "t-4" }).stdout).systemMessage).toContain("Handing over");
      reset();
    },
    T,
  );

  test(
    "turn-start snapshots the dirty files at the start of the turn: paths, hashes and a copy of each",
    () => {
      put(repo, "app/wip.py", "X = 1\n");
      hook("turn-start.sh", { session_id: "t-5" });
      const snap = join(repo, ".forge/state/t-5.turn");
      expect(readFileSync(join(snap, "head"), "utf8").trim()).toBe(git(repo, "rev-parse", "HEAD").stdout.trim());
      expect(readFileSync(join(snap, "dirty"), "utf8")).toBe(`app/wip.py\t${git(repo, "hash-object", "app/wip.py").stdout.trim()}\n`);
      expect(readFileSync(join(snap, "files/app/wip.py"), "utf8")).toBe("X = 1\n");
      reset();
    },
    T,
  );

  /** @rule python/sqlalchemy/no-query-api */
  test(
    "Stop checks only what changed during the turn: the user's broken work in another file does not block a clean change",
    () => {
      put(repo, "app/wip.py", "def f(session):\n    return session.query(1)\n");
      hook("turn-start.sh", { session_id: "t-6" });
      put(repo, "app/ok.py", "X = 1\n");
      expect(hook("stop.sh", { session_id: "t-6" }).stdout).toBe("");
      reset();
    },
    T,
  );

  /** @rule python/sqlalchemy/no-query-api */
  test(
    "in a file the user had already changed, only the lines added during the turn count",
    () => {
      put(repo, "app/wip.py", "def f(session):\n    return session.query(1)\n");
      hook("turn-start.sh", { session_id: "t-7" });
      put(repo, "app/wip.py", "def f(session):\n    return session.query(1)\n\n\nX = 1\n");
      expect(hook("stop.sh", { session_id: "t-7" }).stdout).toBe("");
      hook("turn-start.sh", { session_id: "t-7" });
      put(repo, "app/wip.py", "def f(session):\n    return session.query(1)\n\n\ndef g(session):\n    return session.query(2)\n");
      const reason = JSON.parse(hook("stop.sh", { session_id: "t-7" }).stdout).reason;
      expect(reason).toContain("forge:python/sqlalchemy/no-query-api app/wip.py:6:12");
      expect(reason).not.toContain("app/wip.py:2:12");
      reset();
    },
    T,
  );

  test(
    "a commit made during the turn is still part of the turn",
    () => {
      hook("turn-start.sh", { session_id: "t-8" });
      put(repo, "app/q.py", "def f(session):\n    return session.query(1)\n");
      git(repo, "add", "-A");
      git(repo, "commit", "-qm", "q", "--no-verify");
      expect(JSON.parse(hook("stop.sh", { session_id: "t-8" }).stdout).reason).toContain("forge:python/sqlalchemy/no-query-api app/q.py:2:12");
      git(repo, "reset", "-q", "--hard", "HEAD~1");
    },
    T,
  );

  /** @rule python/sqlalchemy/no-query-api */
  test(
    "post-edit reports findings only on lines added during the turn",
    () => {
      put(repo, "app/wip.py", "def f(session):\n    return session.query(1)\n");
      hook("turn-start.sh", { session_id: "t-9" });
      put(repo, "app/wip.py", "def f(session):\n    return session.query(1)\n\n\nX = 1\n");
      expect(hook("post-edit.sh", { session_id: "t-9", ...edit("app/wip.py") }).stdout).toBe("");
      put(repo, "app/wip.py", "def f(session):\n    return session.query(1)\n\n\ndef g(session):\n    return session.query(2)\n");
      const ctx = JSON.parse(hook("post-edit.sh", { session_id: "t-9", ...edit("app/wip.py") }).stdout).hookSpecificOutput.additionalContext;
      expect(ctx).toContain("app/wip.py:6:12");
      expect(ctx).not.toContain("app/wip.py:2:12");
      reset();
    },
    T,
  );

  /** @rule python/typecheck */
  test(
    "typecheck wakes Claude only for errors in files changed during the turn",
    () => {
      stub("basedpyright", 'echo "app/wip.py:1:1 - error: nope"; exit 1');
      put(repo, "app/wip.py", "X = 1\n");
      hook("turn-start.sh", { session_id: "t-10" });
      put(repo, "app/main.py", readFileSync(join(repo, "app/main.py"), "utf8") + "\n# edited\n");
      const r = hook("typecheck.sh", { session_id: "t-10", ...edit("app/main.py") });
      expect(r.status).toBe(0);
      expect(r.stderr).toBe("");
      rmSync(join(repo, ".venv/bin/basedpyright"));
      reset();
    },
    T,
  );

  /** @rule python/pytest/no-test-edits */
  test(
    "a test that existed at the start of the turn and was edited blocks Stop; a new test in the same file does not; pre-commit does not check it",
    () => {
      const before = readFileSync(join(repo, "tests/test_main.py"), "utf8");
      const more = "\n\ndef test_more() -> None:\n    assert 1 + 1 == 2\n";
      hook("turn-start.sh", { session_id: "t-11" });
      put(repo, "tests/test_main.py", before + more);
      expect(hook("stop.sh", { session_id: "t-11" }).stdout).toBe("");
      hook("turn-start.sh", { session_id: "t-11" });
      put(repo, "tests/test_main.py", before.replace(/assert (.*)$/m, "assert True or $1") + more);
      const reason = JSON.parse(hook("stop.sh", { session_id: "t-11" }).stdout).reason;
      expect(reason).toContain('forge:python/pytest/no-test-edits failed\ntests/test_main.py: test "test_health" was changed');
      expect(sh("sh", [".forge/check.sh", "--changed"], { cwd: repo }).stdout).not.toContain("no-test-edits");
      reset();
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

/** @pack typescript/bun */
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

/**
 * Errors that were already in files Claude did not touch are not Claude's: the
 * Stop hook and the background type check report only errors in changed files.
 */
describe("errors in unchanged files", () => {
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
    "Stop allows a clean edit while an unchanged file has a type error",
    () => {
      const r = legacyRepo();
      put(r, "src/ok.ts", "export const ok: number = 1;\n");
      expect(hook("stop.sh", { session_id: "legacy" }, {}, r).stdout).toBe("");
    },
    T,
  );

  /** @rule typescript/typecheck */
  test.if(hasBun)(
    "typecheck stays silent for errors only in unchanged files, and wakes Claude for an error in the edited file",
    () => {
      const r = legacyRepo();
      const edit = { tool_name: "Edit", tool_input: { file_path: join(r, "src/ok.ts") } };
      put(r, "src/ok.ts", "export const ok: number = 1;\n");
      const quiet = hook("typecheck.sh", edit, {}, r);
      expect(quiet.status).toBe(0);
      expect(quiet.stderr).toBe("");
      put(r, "src/ok.ts", 'export const ok: number = "no";\n');
      const loud = hook("typecheck.sh", edit, {}, r);
      expect(loud.status).toBe(2);
      expect(loud.stderr).toContain("forge:typescript/typecheck\nsrc/ok.ts(1,14): error TS2322");
      expect(loud.stderr).not.toContain("legacy");
    },
    T,
  );
});

