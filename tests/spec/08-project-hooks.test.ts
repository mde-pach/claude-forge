/**
 * @spec 8 · Project hooks
 *
 * At project level Forge writes a `forge-project` plugin whose hooks run with
 * plain `sh`. They read paths and the working directory from the hook input,
 * never from `CLAUDE_PROJECT_DIR`, and print nothing on success. These tests
 * run the generated scripts on the sample FastAPI repository with real hook
 * JSON; ast-grep runs through `bunx` or `uvx`.
 */
import { afterEach, beforeAll, describe, expect, test } from "bun:test";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, realpathSync, rmSync, statSync, symlinkSync, utimesSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { renderChecks } from "../../src/render";
import { applied, commitAll, copyRepo, ensureTools, fixture, forge, git, hasBun, hasUv, hookJson, linkTools, put, sampleRepo, sh, stateDir, stopEngines } from "../helpers";

const T = 180_000;
const SCRIPTS = ".claude/skills/forge-project/scripts";
let repo = "";

function hook(script: string, fields: Record<string, unknown>, env: Record<string, string> = {}, cwd = repo) {
  // an interactive session unless a test says otherwise (the suite may itself run under claude -p)
  return sh("sh", [join(SCRIPTS, script)], { cwd, input: hookJson({ cwd, ...fields }), env: { CLAUDE_CODE_ENTRYPOINT: "cli", ...env } });
}
/**
 * Back to the committed tree, without what the hooks left: untracked files, the
 * ignored ones (.forge/state/ tool caches), all but the .venv the tests put tools
 * in, and the hook state outside the repository (stateDir: last-pass, last-fail,
 * turn snapshots, review cache). Runs after every test, so no test depends on
 * what an earlier one left, failed or not.
 */
function reset() {
  if (!repo) return;
  // SessionStart starts the types engines (§15.3): none outlives the test
  stopEngines(repo);
  git(repo, "reset", "-q", "--hard");
  git(repo, "clean", "-qfdx", "-e", ".venv");
  rmSync(stateDir(repo), { recursive: true, force: true });
}
afterEach(reset);
/** Runs <fn> with the real ty (Forge's pinned one, through uvx): without the repository's ty stand-in, put back afterwards. */
function withRealTy(fn: () => void) {
  const p = join(repo, ".venv", "bin", "ty-stand-in");
  const saved = existsSync(p) ? readFileSync(p, "utf8") : undefined;
  rmSync(p, { force: true });
  try {
    fn();
  } finally {
    if (saved !== undefined) {
      writeFileSync(p, saved);
      chmodSync(p, 0o755);
    }
  }
}
/** A stand-in for a project tool in the repository's .venv. */
function stubTool(name: string, body: string) {
  const p = join(repo, ".venv", "bin", name);
  writeFileSync(p, `#!/bin/sh\n${body}\n`);
  chmodSync(p, 0o755);
}
/** Runs <fn> with a stand-in for a tool in the repository's .venv (ty's: ty-stand-in), then puts back what was there. */
function withStub(name: string, body: string, fn: () => void) {
  const p = join(repo, ".venv", "bin", name);
  const saved = existsSync(p) ? readFileSync(p, "utf8") : undefined;
  stubTool(name, body);
  try {
    fn();
  } finally {
    rmSync(p, { force: true });
    if (saved !== undefined) {
      writeFileSync(p, saved);
      chmodSync(p, 0o755);
    }
  }
}
/**
 * A ty that takes a second, then reports a type error in app/main.py, for the tests
 * of when typecheck.sh runs (its lock, headless sessions), where the tool's own output
 * is not under test: they do not depend on how the real one judges the sample.
 */
const FAILING_TYPECHECK = 'sleep 1; echo "app/main.py:1:1 - error: stand-in type error"; exit 1';
/** Runs <fn> with the repository's .venv/bin/ruff replaced by a wrapper around the real one (<body> runs first), restored afterwards. */
function withRuffWrapper(body: string, fn: () => void) {
  const real = join(repo, ".venv/bin/ruff");
  const target = sh("sh", ["-c", `readlink "${real}"`]).stdout.trim();
  rmSync(real);
  writeFileSync(real, `#!/bin/sh\n${body}\nexec "${target}" "$@"\n`);
  chmodSync(real, 0o755);
  try {
    fn();
  } finally {
    rmSync(real);
    symlinkSync(target, real);
  }
}
/** A test added with a clean change, so the change is not "untested" (python/pytest/untested-change). */
const OK_TEST = "def test_ok() -> None:\n    assert 1 + 1 == 2\n";
/** Adds a test to tests/test_main.py, so a clean change made with it passes Stop (python/pytest/untested-change). */
const addTest = () => put(repo, "tests/test_main.py", `${readFileSync(join(repo, "tests/test_main.py"), "utf8")}\n\ndef test_turn_ok() -> None:\n    assert 1 + 1 == 2\n`);
const edit = (rel: string) => ({ tool_name: "Edit", tool_input: { file_path: join(repo, rel) } });

beforeAll(() => {
  repo = applied("fastapi-app");
  commitAll(repo, "forge");
  mkdirSync(join(repo, ".venv", "bin"), { recursive: true });
  const ruff = sh("sh", ["-c", "command -v ruff"]).stdout.trim();
  if (ruff) symlinkSync(ruff, join(repo, ".venv", "bin", "ruff"));
  // ty (Forge's, through uvx) finds nothing here unless a test says otherwise: the sample's
  // dependencies are not installed, and these tests are not about what ty finds
  stubTool("ty-stand-in", "exit 0");
}, T);

test(
  "hooks.json wires SessionStart, UserPromptSubmit, PreToolUse(Bash) twice and PreToolUse(Edit|Write), PermissionRequest, PostToolUse(Edit|Write|Bash) twice and PostToolUse(Edit|Write), PostToolUseFailure(Bash) twice and Stop, with the specified timeouts",
  () => {
    const hooks = JSON.parse(readFileSync(join(repo, ".claude/skills/forge-project/hooks/hooks.json"), "utf8")).hooks;
    expect(hooks.SessionStart[0]).toMatchObject({ matcher: "startup|resume|clear|compact", hooks: [{ timeout: 10 }] });
    expect(hooks.UserPromptSubmit[0]).toMatchObject({ hooks: [{ command: 'sh "${CLAUDE_PLUGIN_ROOT}/scripts/turn-start.sh"', timeout: 10 }] });
    expect(hooks.PreToolUse[0]).toMatchObject({ matcher: "Bash", hooks: [{ timeout: 180, onFailure: "block" }] });
    expect(hooks.PreToolUse[1]).toMatchObject({ matcher: "Bash", hooks: [{ command: 'sh "${CLAUDE_PLUGIN_ROOT}/scripts/bash-snapshot.sh"', timeout: 15 }] });
    expect(hooks.PermissionRequest[0]).toMatchObject({ hooks: [{ command: 'sh "${CLAUDE_PLUGIN_ROOT}/scripts/review.sh"', timeout: 180 }] });
    expect(hooks.PostToolUse[0].matcher).toBe("Edit|Write|Bash");
    expect(hooks.PreToolUse[2]).toMatchObject({ matcher: "Edit|Write", hooks: [{ command: 'sh "${CLAUDE_PLUGIN_ROOT}/scripts/test-edit.sh"', timeout: 180 }] });
    expect(hooks.PostToolUse[0].hooks).toMatchObject([{ timeout: 60 }, { timeout: 300, asyncRewake: true }]);
    expect(hooks.PostToolUse[1]).toMatchObject({ matcher: "Edit|Write", hooks: [{ command: 'sh "${CLAUDE_PLUGIN_ROOT}/scripts/test-edit.sh"', timeout: 10 }] });
    expect(hooks.PostToolUseFailure[0].matcher).toBe("Bash");
    expect(hooks.PostToolUseFailure[0].hooks).toEqual(hooks.PostToolUse[0].hooks);
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
      expect(out).toContain("forge: packs git, python, python/asyncpg, python/fastapi, python/pydantic, python/pytest, python/sqlalchemy, python/sqlalchemy/alembic, python/uv, secrets, supply-chain");
      reset();
    },
    T,
  );

  test(
    "missing tools are reported once, not at every session start",
    () => {
      rmSync(join(stateDir(repo), "missing-tools"), { force: true });
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
  /** Runs a hook script; verdicts cached by an earlier test are dropped unless `keepCache`. */
  function run(script: string, fields: Record<string, unknown>, env: Record<string, string>, keepCache = false) {
    rmSync(log(), { force: true });
    if (!keepCache) rmSync(join(stateDir(ts), "review-cache"), { force: true });
    return sh("sh", [join(SCRIPTS, script)], {
      cwd: ts,
      input: hookJson({ cwd: ts, permission_mode: "default", ...fields }),
      env: {
        PATH: `${bin}:${process.env.PATH}`,
        FAKE_LOG: log(),
        FAKE_REASON: "it is routine",
        FAKE_DECISION: "allow",
        CLAUDE_CODE_ENTRYPOINT: "sdk-cli",
        FORGE_ASK_REVIEW: "",
        FORGE_REVIEWER: "",
        ...env,
      },
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
      expect(readFileSync(join(stateDir(ts), "reviews.log"), "utf8")).toContain("\tdeny\tBash\t");
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
      for (const arg of [
        "--tools\nARG:Read,Grep,Glob",
        "--permission-mode\nARG:dontAsk",
        '--settings\nARG:{"disableAllHooks":true}',
        "--setting-sources\nARG:\n",
        "--strict-mcp-config",
        "--no-session-persistence",
        "--json-schema",
      ])
        expect(call).toContain(arg);
      expect(call).toContain('Input: {"command":"npm install left-pad"}');
      expect(call).toContain("Forge flagged it with rule typescript/bun/no-other-pm.");
      expect(call).not.toContain("/secret/transcript.jsonl");
    },
    T,
  );

  /** @rule safety/ephemeral-exec */
  test(
    "the reviewer of a rule's ask judges only that rule's concern: its description, the check's finding and the rule's review question, not the rest of the command",
    () => {
      const g = mkdtempSync(join(tmpdir(), "forge-guard-review-"));
      for (const f of ["guard.sh", "review.sh", "lib.sh"]) writeFileSync(join(g, f), readFileSync(join(ts, SCRIPTS, f)));
      writeFileSync(join(g, "guard.rules"), "safety/ephemeral-exec\task\tnpx\tPackages fetched on the fly.\tephemeral-exec\tIs that exact package legitimate?\n");
      rmSync(log(), { force: true });
      const out = sh("sh", [join(g, "guard.sh")], {
        cwd: ts,
        input: hookJson({
          cwd: ts,
          session_id: "s-review",
          permission_mode: "default",
          hook_event_name: "PreToolUse",
          tool_name: "Bash",
          tool_input: { command: "git stash && npx some-typo; git stash pop" },
        }),
        env: { PATH: `${bin}:${process.env.PATH}`, FAKE_LOG: log(), FAKE_REASON: "unknown name", FAKE_DECISION: "deny", CLAUDE_CODE_ENTRYPOINT: "sdk-cli", FORGE_ASK_REVIEW: "", FORGE_REVIEWER: "" },
      }).stdout;
      expect(decision(out).permissionDecision).toBe("deny");
      const call = readFileSync(log(), "utf8");
      expect(call).toContain("Forge flagged it with rule safety/ephemeral-exec. The concern of that rule: Packages fetched on the fly. Here: some-typo is not installed where the command runs");
      expect(call).toContain("The question you answer: Is that exact package legitimate?");
      expect(call).toContain("Judge only that question. The rest of the action is not under review");
    },
    T,
  );

  test(
    "a verdict is reused for the same rule and input (whitespace aside) within a session",
    () => {
      const first = { ...npm, session_id: "s-cache", tool_input: { command: "npm install  left-pad" } };
      expect(decision(run("guard.sh", first, { FAKE_DECISION: "deny", FAKE_REASON: "Bun project" }).stdout).permissionDecision).toBe("deny");
      expect(existsSync(log())).toBe(true);
      const again = run("guard.sh", { ...first, tool_input: { command: "npm install left-pad" } }, { FAKE_DECISION: "allow" }, true);
      expect(decision(again.stdout).permissionDecision).toBe("deny");
      expect(existsSync(log())).toBe(false);
      expect(decision(run("guard.sh", { ...first, session_id: "s-other" }, { FAKE_DECISION: "allow" }, true).stdout).permissionDecision).toBe("allow");
    },
    T,
  );

  test(
    "a verdict on a test edit is not reused once the request for the turn changed",
    () => {
      const edit = { hook_event_name: "PreToolUse", session_id: "s-req", tool_name: "Edit", tool_input: { file_path: join(ts, "a.test.ts"), old_string: "1", new_string: "2" } };
      const pre = (request: string, d: string, keep: boolean) => {
        rmSync(log(), { force: true });
        if (!keep) rmSync(join(stateDir(ts), "review-cache"), { force: true });
        return decision(
          sh("sh", [join(SCRIPTS, "review.sh"), "pre", "typescript/vitest/no-test-edits", "changes a test", "Is it required?", request], {
            cwd: ts,
            input: hookJson({ cwd: ts, permission_mode: "default", ...edit }),
            env: { PATH: `${bin}:${process.env.PATH}`, FAKE_LOG: log(), FAKE_REASON: "r", FAKE_DECISION: d, CLAUDE_CODE_ENTRYPOINT: "sdk-cli", FORGE_ASK_REVIEW: "", FORGE_REVIEWER: "" },
          }).stdout,
        ).permissionDecision;
      };
      expect(pre("Totals exclude the fee now: update the tests.", "allow", false)).toBe("allow");
      expect(pre("Totals exclude the fee now: update the tests.", "deny", true)).toBe("allow");
      expect(existsSync(log())).toBe(false);
      expect(pre("Fix the failing build.", "deny", true)).toBe("deny");
      expect(existsSync(log())).toBe(true);
    },
    T,
  );

  /** @rule safety/forge-state */
  test(
    "the hooks' state (review verdicts and log, the saved request, turn snapshots) lives outside the repository, readable by the user only, so the agent's Bash cannot forge it",
    () => {
      rmSync(join(ts, ".forge/state"), { recursive: true, force: true });
      rmSync(stateDir(ts), { recursive: true, force: true });
      run("turn-start.sh", { hook_event_name: "UserPromptSubmit", session_id: "s-state", prompt: "Add a cart total." }, {});
      run("guard.sh", { ...npm, session_id: "s-state" }, { FAKE_DECISION: "deny" });
      expect(readFileSync(join(stateDir(ts), "s-state.prompt"), "utf8")).toBe("Add a cart total.");
      expect(existsSync(join(stateDir(ts), "s-state.turn/head"))).toBe(true);
      expect(readFileSync(join(stateDir(ts), "review-cache"), "utf8")).toContain("s-state\t");
      expect(readFileSync(join(stateDir(ts), "reviews.log"), "utf8")).toContain("\tdeny\tBash\t");
      expect(statSync(stateDir(ts)).mode & 0o077).toBe(0);
      expect(existsSync(join(ts, ".forge/state"))).toBe(false);
      // a verdict planted where the cache used to be is not read
      mkdirSync(join(ts, ".forge/state"), { recursive: true });
      writeFileSync(join(ts, ".forge/state/review-cache"), readFileSync(join(stateDir(ts), "review-cache"), "utf8").replace("\tdeny\t", "\tallow\t"));
      rmSync(join(stateDir(ts), "review-cache"));
      expect(decision(run("guard.sh", { ...npm, session_id: "s-state" }, { FAKE_DECISION: "deny" }, true).stdout).permissionDecision).toBe("deny");
      rmSync(join(ts, ".forge/state"), { recursive: true, force: true });
    },
    T,
  );

  test.each([
    ["an interactive session (entrypoint cli): the usual prompt", { CLAUDE_CODE_ENTRYPOINT: "cli" }, {}],
    ["FORGE_ASK_REVIEW=never", { FORGE_ASK_REVIEW: "never" }, {}],
    ["dontAsk mode (the user chose to deny asks)", {}, { permission_mode: "dontAsk" }],
    ["inside a review (FORGE_REVIEWER set)", { FORGE_REVIEWER: "1" }, {}],
  ])(
    "no review for %s: the guard keeps its ask",
    (_label, env, fields) => {
      expect(decision(run("guard.sh", { ...npm, ...fields }, env).stdout).permissionDecision).toBe("ask");
      expect(existsSync(log())).toBe(false);
    },
    T,
  );

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

/**
 * Agents often change files through Bash (a heredoc, `sed -i`, `git apply`)
 * rather than Edit or Write. Before each Bash command `bash-snapshot.sh` records
 * the working tree as a git tree (keyed by `tool_use_id`, through a copy of the
 * index and a private object directory); after it, post-edit.sh and typecheck.sh
 * check the files the command added or modified.
 */
describe("files changed by Bash", () => {
  const bash = (id: string, extra: Record<string, unknown> = {}) => ({ tool_name: "Bash", tool_use_id: id, tool_input: { command: "python3 - <<EOF" }, ...extra });
  const objects = () => git(repo, "count-objects").stdout;

  /** @rule python/sqlalchemy/no-declarative-base */
  test(
    "a Bash command that writes a forbidden pattern into a new untracked file gets the finding as additionalContext; the user's index and object store are untouched",
    () => {
      put(repo, "app/staged.py", "X = 1\n");
      git(repo, "add", "app/staged.py");
      const index = readFileSync(join(repo, ".git/index"));
      const before = objects();
      expect(hook("bash-snapshot.sh", bash("toolu_b1")).stdout).toBe("");
      put(repo, "app/via_bash.py", "def f():\n    return declarative_base(1)\n");
      const out = JSON.parse(hook("post-edit.sh", bash("toolu_b1")).stdout).hookSpecificOutput;
      expect(out.additionalContext).toContain("forge:python/sqlalchemy/no-declarative-base app/via_bash.py:2:12");
      expect(readFileSync(join(repo, ".git/index")).equals(index)).toBe(true);
      expect(objects()).toBe(before);
      expect(existsSync(join(stateDir(repo), "bash/objects"))).toBe(false);
      reset();
    },
    T,
  );

  /** @rule python/sqlalchemy/no-declarative-base */
  test(
    "a modified tracked file is checked on the lines the turn added; a command that changes nothing prints nothing",
    () => {
      hook("turn-start.sh", { session_id: "b-2" });
      hook("bash-snapshot.sh", bash("toolu_b2", { session_id: "b-2" }));
      expect(hook("post-edit.sh", bash("toolu_b2", { session_id: "b-2" })).stdout).toBe("");
      hook("bash-snapshot.sh", bash("toolu_b3", { session_id: "b-2" }));
      put(repo, "app/main.py", readFileSync(join(repo, "app/main.py"), "utf8") + "\n\ndef q():\n    return declarative_base(2)\n");
      expect(JSON.parse(hook("post-edit.sh", bash("toolu_b3", { session_id: "b-2" })).stdout).hookSpecificOutput.additionalContext).toContain(
        "forge:python/sqlalchemy/no-declarative-base app/main.py:",
      );
      reset();
    },
    T,
  );

  /** @rule python/sqlalchemy/no-declarative-base python/typecheck */
  test.if(hasUv)(
    "a Bash command that fails or is interrupted after writing files is checked like one that succeeded (PostToolUseFailure)",
    () =>
      withRealTy(() => {
        const failed = (id: string, extra: Record<string, unknown> = {}) => bash(id, { hook_event_name: "PostToolUseFailure", error: "Exit code 3", is_interrupt: false, ...extra });
        hook("bash-snapshot.sh", bash("toolu_f1"));
        put(repo, "app/partial_a.py", "def f():\n    return declarative_base(1)\n");
        put(repo, "app/partial_b.py", 'X: int = "no"\n');
        const out = JSON.parse(hook("post-edit.sh", failed("toolu_f1")).stdout).hookSpecificOutput;
        expect(out.hookEventName).toBe("PostToolUseFailure");
        expect(out.additionalContext).toContain("forge:python/sqlalchemy/no-declarative-base app/partial_a.py:2:12");
        const r = hook("typecheck.sh", failed("toolu_f1"));
        expect(r.status).toBe(2);
        expect(r.stderr).toContain('app/partial_b.py:1:10: error[invalid-assignment] Object of type `Literal["no"]` is not assignable to `int`');
        expect(readdirSync(join(stateDir(repo), "bash")).filter((f) => f.startsWith("toolu_f1"))).toEqual([]);
        hook("bash-snapshot.sh", bash("toolu_f2"));
        put(repo, "app/interrupted.py", "def g():\n    return declarative_base(2)\n");
        expect(JSON.parse(hook("post-edit.sh", failed("toolu_f2", { error: "Interrupted", is_interrupt: true })).stdout).hookSpecificOutput.additionalContext).toContain("app/interrupted.py:2:12");
      }),
    T,
  );

  /**
   * A tool can cost seconds per run (Biome scanning a large monorepo took 17 to 25 s per
   * call), so each runs once for all the files a command changed, and the checks stop
   * after `FORGE_EDIT_BUDGET` (45 s, under the hook's 60 s timeout) with what they found.
   */
  /** @rule python/format python/lint python/sqlalchemy/no-declarative-base */
  test(
    "after a Bash command that changed several files, each tool runs once on all of them, and each file's findings are reported",
    () => {
      const log = join(mkdtempSync(join(tmpdir(), "forge-ruff-")), "calls");
      withRuffWrapper(`printf '%s\\n' "$*" >> "${log}"`, () => {
        hook("bash-snapshot.sh", bash("toolu_m1"));
        put(repo, "app/m1.py", "import os\n\ndef f():\n    return declarative_base(1)\n");
        put(repo, "app/m2.py", "import sys\n\ndef g():\n    return declarative_base(2)\n");
        const ctx = JSON.parse(hook("post-edit.sh", bash("toolu_m1")).stdout).hookSpecificOutput.additionalContext;
        expect(ctx).toContain("forge:python/sqlalchemy/no-declarative-base app/m1.py:5:12");
        expect(ctx).toContain("forge:python/sqlalchemy/no-declarative-base app/m2.py:5:12");
        expect(ctx).toContain("app/m1.py:1:8: F401");
        expect(ctx).toContain("app/m2.py:1:8: F401");
        const calls = readFileSync(log, "utf8").trim().split("\n");
        expect(calls).toEqual([
          "format app/m1.py app/m2.py",
          "check --fix --fixable I,UP --exit-zero --show-fixes --output-format concise app/m1.py app/m2.py",
          "check --output-format concise app/m1.py app/m2.py",
        ]);
      });
    },
    T,
  );

  /** @rule python/sqlalchemy/no-declarative-base */
  test(
    "checks still running after FORGE_EDIT_BUDGET seconds are stopped, and Claude gets what was found with a note",
    () =>
      withRuffWrapper('case "$*" in "check --output-format"*) sleep 60 ;; esac', () => {
        put(repo, "app/slow.py", "X = 1\n");
        const started = Date.now();
        const r = hook("post-edit.sh", edit("app/slow.py"), { FORGE_EDIT_BUDGET: "2" });
        // a hook that waited for the slow lint would take its whole 60 s: anything less means it was stopped
        expect(Date.now() - started).toBeLessThan(60_000);
        expect(JSON.parse(r.stdout).hookSpecificOutput.additionalContext).toContain("forge: the checks after this edit stopped after 2s");
      }),
    T,
  );

  test(
    "files under .forge/, .claude/, .git/, node_modules/ and .venv/ are not checked, and deleted files are ignored",
    () => {
      hook("bash-snapshot.sh", bash("toolu_b4"));
      put(repo, "node_modules/pkg/q.py", "def f():\n    return declarative_base(1)\n");
      rmSync(join(repo, "app/main.py"));
      expect(hook("post-edit.sh", bash("toolu_b4")).stdout).toBe("");
      reset();
    },
    T,
  );

  /** @rule python/typecheck */
  test.if(hasUv)(
    "typecheck.sh runs when a Bash command changed a file in a typecheck's scope, using the list post-edit.sh wrote",
    () =>
      withRealTy(() => {
        hook("bash-snapshot.sh", bash("toolu_b5"));
        put(repo, "app/typed.py", 'X: int = "no"\n');
        hook("post-edit.sh", bash("toolu_b5"));
        const r = hook("typecheck.sh", bash("toolu_b5"));
        expect(r.status).toBe(2);
        expect(r.stderr).toContain("forge:python/typecheck");
        expect(r.stderr).toContain('app/typed.py:1:10: error[invalid-assignment] Object of type `Literal["no"]` is not assignable to `int`');
        expect(existsSync(join(stateDir(repo), "bash/toolu_b5.files"))).toBe(false);
      }),
    T,
  );

  test(
    "snapshots of commands that never finished (denied by the guard) are cleaned: older than a day, or beyond the 50 newest",
    () => {
      mkdirSync(join(stateDir(repo), "bash"), { recursive: true });
      for (let i = 0; i < 60; i++) writeFileSync(join(stateDir(repo), `bash/stale${i}.tree`), "x\n");
      hook("bash-snapshot.sh", bash("toolu_b6"));
      const left = readdirSync(join(stateDir(repo), "bash")).filter((f) => f.endsWith(".tree"));
      expect(left.length).toBeLessThanOrEqual(51);
      expect(left).toContain("toolu_b6.tree");
      rmSync(join(stateDir(repo), "bash"), { recursive: true, force: true });
    },
    T,
  );

  test(
    "when the tree is too large to snapshot in time, the change is left to Stop, with a note given once",
    () => {
      mkdirSync(join(stateDir(repo), "bash"), { recursive: true });
      rmSync(join(stateDir(repo), "bash/snapshot-skipped"), { force: true });
      writeFileSync(join(stateDir(repo), "bash/toolu_b7.tree"), "skipped\n");
      expect(JSON.parse(hook("post-edit.sh", bash("toolu_b7")).stdout).hookSpecificOutput.additionalContext).toContain("the Stop check still covers them");
      writeFileSync(join(stateDir(repo), "bash/toolu_b8.tree"), "skipped\n");
      expect(hook("post-edit.sh", bash("toolu_b8")).stdout).toBe("");
      rmSync(join(stateDir(repo), "bash"), { recursive: true, force: true });
    },
    T,
  );

  test(
    "outside a git repository nothing is snapshotted",
    () => {
      const plain = mkdtempSync(join(tmpdir(), "forge-nogit-"));
      const r = sh("sh", [join(repo, SCRIPTS, "bash-snapshot.sh")], { cwd: plain, input: hookJson({ cwd: plain, ...bash("toolu_b9") }) });
      expect(r.status).toBe(0);
      expect(r.stdout).toBe("");
      expect(existsSync(join(plain, ".forge"))).toBe(false);
    },
    T,
  );
});

/**
 * After an edit, a lint check's safe fixes (`fix_run`) run before the report, so
 * the agent does not spend a turn on sorted imports. Unused imports and variables
 * are never removed: an import added now is often used in the next edit.
 */
describe("safe fixes after an edit", () => {
  const post = (r: string, fields: Record<string, unknown>) => sh("sh", [join(SCRIPTS, "post-edit.sh")], { cwd: r, input: hookJson({ cwd: r, ...fields }) });
  const ctx = (out: string) => JSON.parse(out).hookSpecificOutput.additionalContext as string;

  /** @rule python/lint */
  test.if(hasUv)(
    "Python: sorted imports and pyupgrade fixes are applied when the project selects I and UP, said in one line; F401 is still reported, not fixed; the same after a Bash command",
    () => {
      ensureTools();
      const r = sampleRepo("fastapi-app");
      linkTools(r, "python");
      put(r, "pyproject.toml", readFileSync(join(r, "pyproject.toml"), "utf8").replace('select = ["E", "F", "ANN401", "E722"]', 'select = ["E", "F", "ANN401", "E722", "I", "UP"]'));
      commitAll(r, "config");
      expect(forge(["apply", "--yes"], r).status).toBe(0);
      commitAll(r, "forge");
      const messy = "import sys\nimport os\nfrom typing import List\n\n\ndef f(x: List[int]) -> None:\n    print(os.sep, sys.argv, x)\n";
      put(r, "app/x.py", messy);
      const out = ctx(post(r, { tool_name: "Edit", tool_input: { file_path: join(r, "app/x.py") } }).stdout);
      expect(out).toContain("forge: auto-fixed UP006, I001 in app/x.py; re-read it before editing it again");
      expect(out).toContain("F401");
      expect(out).not.toContain("I001 [*]");
      expect(readFileSync(join(r, "app/x.py"), "utf8")).toStartWith("import os\nimport sys\n");
      sh("sh", [join(SCRIPTS, "bash-snapshot.sh")], { cwd: r, input: hookJson({ cwd: r, tool_name: "Bash", tool_use_id: "toolu_fx", tool_input: { command: "cat > app/y.py" } }) });
      put(r, "app/y.py", messy);
      expect(ctx(post(r, { tool_name: "Bash", tool_use_id: "toolu_fx", tool_input: { command: "cat > app/y.py" } }).stdout)).toContain("forge: auto-fixed UP006, I001 in app/y.py");
    },
    T,
  );

  /** @rule typescript/format-lint */
  test.if(hasBun)(
    "TypeScript: Biome's safe fixes are applied without formatting and said in one line; an unused import is kept",
    () => {
      ensureTools();
      const r = sampleRepo("nextjs-prisma-app");
      linkTools(r, "node");
      commitAll(r, "tools");
      expect(forge(["apply", "--yes"], r).status).toBe(0);
      commitAll(r, "forge");
      put(r, "src/fix.ts", 'import { join } from "node:path";\nlet total = 1;\nexport const sum = total + 1;\n');
      const out = ctx(post(r, { tool_name: "Write", tool_input: { file_path: join(r, "src/fix.ts") } }).stdout);
      expect(out).toContain("forge: auto-fixed safe lint fixes in src/fix.ts");
      const text = readFileSync(join(r, "src/fix.ts"), "utf8");
      expect(text).toContain("const total = 1;");
      expect(text).toContain('import { join } from "node:path";');
    },
    T,
  );

  test(
    "a clean edit prints nothing: no fix, no note",
    () => {
      put(repo, "app/ok2.py", "X = 1\n");
      expect(hook("post-edit.sh", edit("app/ok2.py")).stdout).toBe("");
      reset();
    },
    T,
  );
});

describe("PostToolUse", () => {
  /** @rule python/format python/lint python/sqlalchemy/no-declarative-base */
  test(
    "post-edit formats the file silently, then returns lint and ast-grep findings for this file as additionalContext (≤ 20 lines)",
    () => {
      put(repo, "app/q.py", "import os\n\ndef f():\n    return declarative_base(1)\n");
      const r = hook("post-edit.sh", edit("app/q.py"));
      expect(r.status).toBe(0);
      const out = JSON.parse(r.stdout).hookSpecificOutput;
      expect(out.hookEventName).toBe("PostToolUse");
      expect(out.additionalContext).toContain("forge:python/sqlalchemy/no-declarative-base app/q.py:5:12");
      if (!existsSync(join(repo, ".venv/bin/ruff"))) throw new Error("ruff is required on PATH for this test");
      expect(out.additionalContext).toContain("F401");
      expect(readFileSync(join(repo, "app/q.py"), "utf8")).toBe("import os\n\n\ndef f():\n    return declarative_base(1)\n");
      expect(out.additionalContext.split("\n").length).toBeLessThanOrEqual(20);
      reset();
    },
    T,
  );

  /**
   * `advise` rules are heuristics: their matches come back after the edit like
   * `forbid` matches, but `check.sh` (Stop, pre-commit, CI) does not run them. No
   * pack rule uses the mode now, so a project-local rule exercises it.
   */
  test(
    "an advise rule's match is returned after the edit, and check.sh does not fail on it",
    () => {
      put(
        repo,
        ".forge/rules/demo-advise.yaml",
        [
          "id: local/demo-advise",
          "description: Calls to httpx.get get advice.",
          "scope: {language: python}",
          "enforce:",
          "  - target: ast-grep",
          "    mode: advise",
          "    rule: {pattern: httpx.get($U)}",
          "examples:",
          "  match: ['httpx.get(url)']",
          "  no_match: ['httpx.post(url)']",
          "",
        ].join("\n"),
      );
      try {
        expect(forge(["apply", "--yes"], repo).status).toBe(0);
        put(repo, "app/preview.py", "import httpx\n\n\ndef preview(url: str) -> str:\n    return httpx.get(url).text\n");
        const out = JSON.parse(hook("post-edit.sh", edit("app/preview.py")).stdout).hookSpecificOutput;
        expect(out.additionalContext).toContain("forge:local/demo-advise app/preview.py:5:12");
        expect(readFileSync(join(repo, ".forge/ast-grep/rules/local/demo-advise.yml"), "utf8")).toContain("severity: warning");
        const r = sh("sh", [".forge/check.sh", "--all"], { cwd: repo });
        expect(r.stdout + r.stderr).not.toContain("local/demo-advise");
      } finally {
        rmSync(join(repo, ".forge/rules/demo-advise.yaml"), { force: true });
        expect(forge(["apply", "--yes"], repo).status).toBe(0);
        reset();
      }
    },
    T,
  );

  /**
   * A `grep` target in `advise` mode is the same heuristic for text ast-grep does not
   * parse (templates, config): its matches on added lines come back after the edit,
   * and check.sh neither fails on them nor counts them.
   */
  test(
    "a grep advise rule's match on an added line is returned after the edit, and check.sh does not fail on it",
    () => {
      put(
        repo,
        ".forge/rules/demo-grep-advise.yaml",
        [
          "id: local/demo-grep-advise",
          "description: Unescaped output gets advice.",
          "scope: {language: any, files: ['**/*.html']}",
          "enforce:",
          "  - target: grep",
          "    mode: advise",
          "    pattern: '\\|[[:space:]]*safe'",
          "examples:",
          "  match: ['{{ body|safe }}']",
          "  no_match: ['{{ body }}']",
          "",
        ].join("\n"),
      );
      try {
        expect(forge(["apply", "--yes"], repo).status).toBe(0);
        put(repo, "app/templates/page.html", "<p>{{ title }}</p>\n<div>{{ body|safe }}</div>\n");
        const out = JSON.parse(hook("post-edit.sh", edit("app/templates/page.html")).stdout).hookSpecificOutput;
        expect(out.additionalContext).toContain("forge:local/demo-grep-advise app/templates/page.html:2 Unescaped output gets advice.");
        for (const mode of ["--changed", "--all"]) {
          const r = sh("sh", [".forge/check.sh", mode], { cwd: repo });
          expect(r.stdout + r.stderr).not.toContain("local/demo-grep-advise");
        }
      } finally {
        rmSync(join(repo, ".forge/rules/demo-grep-advise.yaml"), { force: true });
        expect(forge(["apply", "--yes"], repo).status).toBe(0);
        reset();
      }
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
    () =>
      withStub("ty-stand-in", FAILING_TYPECHECK, () => {
        put(repo, "app/main.py", readFileSync(join(repo, "app/main.py"), "utf8") + '\nBROKEN: int = "no"\n');
        const input = join(mkdtempSync(join(tmpdir(), "forge-in-")), "in.json");
        writeFileSync(input, hookJson({ cwd: repo, ...edit("app/main.py") }));
        const r = sh("sh", ["-c", `sh ${SCRIPTS}/typecheck.sh <"$1" & sleep 0.3; sh ${SCRIPTS}/typecheck.sh <"$1"; echo "second=$?"; wait $!; echo "first=$?"`, "x", input], { cwd: repo });
        expect(r.stdout).toContain("second=0");
        expect(r.stdout).toContain("first=2");
        expect(r.stderr).toContain("forge:python/typecheck\napp/main.py:1:1 - error: stand-in type error");
        expect(existsSync(join(stateDir(repo), "typecheck.lock"))).toBe(false);
      }),
    T,
  );

  /**
   * Claude Code runs an `asyncRewake` hook in the background only in an interactive
   * session or with streaming input; under `claude -p "<prompt>"` it waits for it, so
   * every edit would wait for a whole-project typecheck (32–64 s per edit in zod). There
   * typecheck.sh exits at once and the Stop check runs the typecheck once.
   * @rule python/typecheck
   */
  test(
    "under claude -p with a prompt (entrypoint sdk-cli, no streaming input) typecheck.sh does not run, leaving the typecheck to Stop; with --input-format stream-json, or FORGE_EDIT_TYPECHECK=always, it runs",
    () =>
      withStub("ty-stand-in", FAILING_TYPECHECK, () => {
        put(repo, "app/main.py", readFileSync(join(repo, "app/main.py"), "utf8") + '\nBROKEN: int = "no"\n');
        const input = join(mkdtempSync(join(tmpdir(), "forge-in-")), "in.json");
        writeFileSync(input, hookJson({ cwd: repo, ...edit("app/main.py") }));
        // each run is a session of its own unless the session's answer is kept
        const rewake = join(stateDir(repo), "test.rewake");
        const run = (env: Record<string, string>, ...args: string[]) => {
          if (!env.KEEP) rmSync(rewake, { force: true });
          return sh("sh", ["-c", 'sh "$0" < "$1"; exit $?', join(SCRIPTS, "typecheck.sh"), input, ...args], { cwd: repo, env: { FORGE_EDIT_TYPECHECK: "", ...env } });
        };
        const headless = run({ CLAUDE_CODE_ENTRYPOINT: "sdk-cli" });
        expect(headless.status).toBe(0);
        expect(headless.stderr).toBe("");
        expect(run({ CLAUDE_CODE_ENTRYPOINT: "sdk-cli" }, "--input-format", "stream-json").status).toBe(2);
        expect(run({ CLAUDE_CODE_ENTRYPOINT: "sdk-cli", FORGE_EDIT_TYPECHECK: "always" }).status).toBe(2);
        expect(run({ CLAUDE_CODE_ENTRYPOINT: "cli" }).status).toBe(2);
        // the ancestors are looked up once per session (session "test" here), then the answer is reused
        expect(run({ CLAUDE_CODE_ENTRYPOINT: "sdk-cli" }, "--input-format", "stream-json").status).toBe(2);
        expect(readFileSync(rewake, "utf8")).toBe("runs\n");
        expect(run({ CLAUDE_CODE_ENTRYPOINT: "sdk-cli", KEEP: "1" }).status).toBe(2);
        writeFileSync(rewake, "blocks\n");
        expect(run({ CLAUDE_CODE_ENTRYPOINT: "sdk-cli", KEEP: "1" }, "--input-format", "stream-json").status).toBe(0);
        rmSync(rewake);
      }),
    T,
  );

  test(
    "typecheck takes over a lock whose process is gone",
    () =>
      withStub("ty-stand-in", FAILING_TYPECHECK, () => {
        put(repo, "app/main.py", readFileSync(join(repo, "app/main.py"), "utf8") + "\nX = 1\n");
        mkdirSync(join(stateDir(repo), "typecheck.lock"), { recursive: true });
        writeFileSync(join(stateDir(repo), "typecheck.lock/pid"), "999999\n");
        // it ran (a skipped typecheck exits 0 silently) and released the lock
        const r = hook("typecheck.sh", edit("app/main.py"));
        expect(r.status).toBe(2);
        expect(r.stderr).toContain("app/main.py:1:1 - error: stand-in type error");
        expect(existsSync(join(stateDir(repo), "typecheck.lock"))).toBe(false);
      }),
    T,
  );

  test(
    "a lock whose pid is not written yet belongs to a typecheck that just started: it is kept, and taken over only once it is a minute old",
    () => {
      const lock = join(stateDir(repo), "typecheck.lock");
      mkdirSync(lock, { recursive: true });
      expect(hook("typecheck.sh", edit("app/main.py")).status).toBe(0);
      expect(existsSync(lock)).toBe(true);
      const old = new Date(Date.now() - 5 * 60_000);
      utimesSync(lock, old, old);
      hook("typecheck.sh", edit("app/main.py"));
      expect(existsSync(lock)).toBe(false);
      reset();
    },
    T,
  );
});

/**
 * An edit that changes what a test asserts, or removes it, when the test existed
 * at the start of the turn, is a real ask (test-edit.sh, PreToolUse Edit|Write):
 * in an interactive session the user sees it; nobody being there, an independent
 * reviewer answers it, given the user's request for the turn. An edit that ran
 * was approved, and the Stop check does not block on it; a change nobody approved
 * (e.g. through Bash) is still blocked at Stop.
 */
describe("test edits", () => {
  const TEST = "tests/test_cart.py";
  const BEFORE = "def test_total():\n    items = [40, 60]\n    assert sum(items) == 100\n";
  const human = { CLAUDE_CODE_ENTRYPOINT: "cli", FORGE_ASK_REVIEW: "", FORGE_REVIEWER: "" };
  function call(session: string, event: string, tool: "Edit" | "Write", input: Record<string, unknown>, env: Record<string, string> = human, id = "tu-1") {
    return hook("test-edit.sh", { session_id: session, hook_event_name: event, tool_name: tool, tool_use_id: id, tool_input: { file_path: join(repo, TEST), ...input } }, env).stdout;
  }
  const ask = (out: string) => (out ? JSON.parse(out).hookSpecificOutput : undefined);
  let head = "";
  function start(session: string, prompt = "Fix the totals.") {
    head = git(repo, "rev-parse", "HEAD").stdout.trim();
    put(repo, TEST, BEFORE);
    git(repo, "add", TEST);
    git(repo, "commit", "-qm", "tests", "--no-verify");
    hook("turn-start.sh", { session_id: session, hook_event_name: "UserPromptSubmit", prompt });
  }
  function finish() {
    git(repo, "reset", "-q", "--hard", head);
    reset();
  }

  /** @rule python/pytest/no-test-edits */
  test(
    "an Edit or Write changing a test that existed at the start of the turn (its assertions, its input data), or removing it, asks and names the test; new tests and reformatting do not",
    () => {
      start("s-te1");
      const changed = ask(call("s-te1", "PreToolUse", "Edit", { old_string: "== 100", new_string: "== 90", replace_all: false }));
      expect(changed.permissionDecision).toBe("ask");
      expect(changed.permissionDecisionReason).toStartWith("forge:python/pytest/no-test-edits This edit changes what tests that existed before this turn assert");
      expect(changed.permissionDecisionReason).toContain('tests/test_cart.py "test_total" (changed)');
      expect(ask(call("s-te1", "PreToolUse", "Edit", { old_string: "items = [40, 60]", new_string: "items = [50, 50]" })).permissionDecisionReason).toContain('"test_total" (changed)');
      expect(call("s-te1", "PreToolUse", "Edit", { old_string: "items = [40, 60]", new_string: "items = [40, 60,]" })).toBe("");
      expect(call("s-te1", "PreToolUse", "Write", { content: BEFORE + "\n\ndef test_empty():\n    assert sum([]) == 0\n" })).toBe("");
      expect(ask(call("s-te1", "PreToolUse", "Write", { content: "def test_other():\n    assert True\n" })).permissionDecisionReason).toContain('"test_total" (removed)');
      // a test added during this turn is Claude's own: changing it asks nothing
      put(repo, TEST, BEFORE + "\n\ndef test_new():\n    assert sum([1]) == 1\n");
      expect(call("s-te1", "PreToolUse", "Edit", { old_string: "sum([1]) == 1", new_string: "sum([1]) == 2" })).toBe("");
      finish();
    },
    T,
  );

  /** @rule python/pytest/no-test-edits */
  test(
    "a test file whose name holds = (test_rate=fee.py) is read as a file, so changing its test asks too",
    () => {
      const t = "test_rate=fee.py";
      head = git(repo, "rev-parse", "HEAD").stdout.trim();
      put(repo, t, BEFORE);
      git(repo, "add", t);
      git(repo, "commit", "-qm", "tests", "--no-verify");
      hook("turn-start.sh", { session_id: "s-eq", hook_event_name: "UserPromptSubmit", prompt: "Fix it." });
      const out = hook(
        "test-edit.sh",
        { session_id: "s-eq", hook_event_name: "PreToolUse", tool_name: "Edit", tool_use_id: "tu-eq", tool_input: { file_path: join(repo, t), old_string: "== 100", new_string: "== 90" } },
        human,
      ).stdout;
      expect(ask(out).permissionDecisionReason).toContain('test_rate=fee.py "test_total" (changed)');
      finish();
    },
    T,
  );

  test(
    "nobody being there, the reviewer answers whether the change is required by the user's request for the turn, which turn-start.sh saved",
    () => {
      const bin = mkdtempSync(join(tmpdir(), "forge-fake-claude-"));
      writeFileSync(
        join(bin, "claude"),
        '#!/bin/sh\nfor a in "$@"; do printf "ARG:%s\\n" "$a"; done > "$FAKE_LOG"\nprintf \'{"structured_output":{"decision":"allow","reason":"the request reverses the total"}}\\n\'\n',
      );
      chmodSync(join(bin, "claude"), 0o755);
      const log = join(bin, "log");
      start("s-te2", "Totals now exclude the 10 EUR fee: update the behaviour and its tests.");
      expect(readFileSync(join(stateDir(repo), "s-te2.prompt"), "utf8")).toBe("Totals now exclude the 10 EUR fee: update the behaviour and its tests.");
      const out = ask(
        call(
          "s-te2",
          "PreToolUse",
          "Edit",
          { old_string: "== 100", new_string: "== 90" },
          { PATH: `${bin}:${process.env.PATH}`, FAKE_LOG: log, CLAUDE_CODE_ENTRYPOINT: "sdk-cli", FORGE_ASK_REVIEW: "", FORGE_REVIEWER: "" },
        ),
      );
      expect(out.permissionDecision).toBe("allow");
      const prompt = readFileSync(log, "utf8");
      expect(prompt).toContain("<request>\nTotals now exclude the 10 EUR fee: update the behaviour and its tests.\n</request>");
      expect(prompt).toContain("The question you answer: Is this change to existing tests required by the user's request");
      finish();
    },
    T,
  );

  test(
    "an edit that ran (PostToolUse) was approved: Stop does not block on it, and still blocks a change nobody approved",
    () => {
      start("s-te3");
      expect(ask(call("s-te3", "PreToolUse", "Edit", { old_string: "== 100", new_string: "== 90" }, human, "tu-ok")).permissionDecision).toBe("ask");
      put(repo, TEST, BEFORE.replace("== 100", "== 90"));
      call("s-te3", "PostToolUse", "Edit", { old_string: "== 100", new_string: "== 90" }, human, "tu-ok");
      expect(readFileSync(join(stateDir(repo), "s-te3.turn/tests-approved"), "utf8")).toBe("tests/test_cart.py\ttest_total\n");
      const stop = (session: string) => hook("stop.sh", { session_id: session }).stdout;
      expect(stop("s-te3")).not.toContain("no-test-edits");
      // the same change in a new turn, made without asking (e.g. through Bash)
      put(repo, TEST, BEFORE);
      hook("turn-start.sh", { session_id: "s-te4", hook_event_name: "UserPromptSubmit", prompt: "x" });
      put(repo, TEST, BEFORE.replace("== 100", "== 90"));
      const blocked = JSON.parse(stop("s-te4")).reason;
      expect(blocked).toContain('tests/test_cart.py: test "test_total" was changed');
      expect(blocked).toContain("make it with Edit or Write: Forge then asks for approval");
      finish();
    },
    T,
  );

  test(
    "changed test support code (a fake, fixture or helper in a test file or conftest.py) is reported once at Stop, naming it, then accepted",
    () => {
      const withMock = "class MockLogfire:\n    def __init__(self):\n        self._variables = {}\n\n\n" + BEFORE;
      head = git(repo, "rev-parse", "HEAD").stdout.trim();
      put(repo, TEST, withMock);
      git(repo, "add", TEST);
      git(repo, "commit", "-qm", "tests", "--no-verify");
      hook("turn-start.sh", { session_id: "s-te6", hook_event_name: "UserPromptSubmit", prompt: "Move _variables to _config." });
      put(repo, TEST, withMock.replace("self._variables", "self._config"));
      const first = JSON.parse(hook("stop.sh", { session_id: "s-te6" }).stdout).reason;
      expect(first).toContain("tests/test_cart.py: test support `MockLogfire` was changed");
      expect(first).toContain("Keep that interface in production (e.g. a property alias for a moved attribute) unless the task removes it");
      expect(hook("stop.sh", { session_id: "s-te6" }).stdout).not.toContain("MockLogfire");
      finish();
    },
    T,
  );

  /** @rule python/pytest/run */
  test(
    "a test file edited during the turn is recorded, and Stop runs it with the related tests even when the edit was reverted since",
    () => {
      start("s-te5");
      call("s-te5", "PostToolUse", "Edit", { old_string: "== 100", new_string: "== 90" }, human, "tu-rev");
      expect(readFileSync(join(stateDir(repo), "s-te5.turn/touched"), "utf8")).toBe("tests/test_cart.py\n");
      const engine = join(mkdtempSync(join(tmpdir(), "forge-engine-")), "engine.sh");
      writeFileSync(engine, renderChecks([]));
      const related = (mode: string) =>
        sh("sh", ["-c", `. "$1"; mode=$2; FORGE_ARG=$3; forge_related_py_tests app/main.py`, "x", engine, mode, join(stateDir(repo), "s-te5.turn")], { cwd: repo }).stdout;
      expect(related("--turn")).toContain("tests/test_cart.py\n");
      expect(related("--changed")).not.toContain("tests/test_cart.py");
      finish();
    },
    T,
  );
});

describe("Stop", () => {
  /** @rule python/sqlalchemy/no-declarative-base */
  test(
    "stop blocks with the check output while check.sh fails, then hands over after 3 blocks",
    () => {
      put(repo, "app/q.py", "def f():\n    return declarative_base(1)\n");
      for (let i = 0; i < 3; i++) {
        const out = JSON.parse(hook("stop.sh", { session_id: "s-1" }).stdout);
        expect(out.decision).toBe("block");
        expect(out.reason).toContain("forge:python/sqlalchemy/no-declarative-base app/q.py:2:12");
      }
      expect(JSON.parse(hook("stop.sh", { session_id: "s-1" }).stdout)).toEqual({ systemMessage: "forge: check failed 3 times. Handing over to you." });
      reset();
    },
    T,
  );

  test(
    "stop remembers a passing tree and does not run the check again until it changes",
    () => {
      put(repo, "app/ok.py", "X = 1\n");
      put(repo, "tests/test_ok.py", OK_TEST);
      expect(hook("stop.sh", { session_id: "s-2" }).stdout).toBe("");
      expect(existsSync(join(stateDir(repo), "last-pass"))).toBe(true);
      // a pytest that fails, put in .venv (ignored: the tree is the same): the check would now fail if it ran
      const runs = join(repo, ".venv", "runs");
      withStub("pytest", `echo run >> "${runs}"; echo "FAILED tests/test_ok.py::test_ok"; exit 1`, () => {
        try {
          expect(hook("stop.sh", { session_id: "s-2" }).stdout).toBe("");
          expect(existsSync(runs)).toBe(false);
          put(repo, "app/ok.py", "X = 2\n");
          expect(JSON.parse(hook("stop.sh", { session_id: "s-2" }).stdout).reason).toContain("FAILED tests/test_ok.py::test_ok");
          expect(existsSync(runs)).toBe(true);
        } finally {
          rmSync(runs, { force: true });
        }
      });
      reset();
      // within a turn, the pass is remembered in the turn's snapshot
      hook("turn-start.sh", { session_id: "s-2" });
      put(repo, "app/ok.py", "X = 1\n");
      put(repo, "tests/test_ok.py", OK_TEST);
      expect(hook("stop.sh", { session_id: "s-2" }).stdout).toBe("");
      expect(existsSync(join(stateDir(repo), "s-2.turn/pass"))).toBe(true);
      reset();
    },
    T,
  );

  test(
    "work the user left uncommitted before the turn does not block Claude when the turn changes nothing",
    () => {
      put(repo, "app/wip.py", "def f():\n    return declarative_base(1)\n");
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
      put(repo, "app/q.py", "def f():\n    return declarative_base(1)\n");
      const out = JSON.parse(hook("stop.sh", { session_id: "t-2" }).stdout);
      expect(out.decision).toBe("block");
      expect(out.reason).toContain("forge:python/sqlalchemy/no-declarative-base app/q.py:2:12");
      reset();
    },
    T,
  );

  test(
    "without a turn-start snapshot (e.g. the first Stop after forge apply) the check runs on every changed file, as before",
    () => {
      rmSync(join(stateDir(repo), "t-3.turn"), { recursive: true, force: true });
      put(repo, "app/wip.py", "def f():\n    return declarative_base(1)\n");
      expect(JSON.parse(hook("stop.sh", { session_id: "t-3" }).stdout).decision).toBe("block");
      reset();
    },
    T,
  );

  test(
    "a new prompt resets the Stop block counter",
    () => {
      put(repo, "app/q.py", "def f():\n    return declarative_base(1)\n");
      for (let i = 0; i < 2; i++) expect(JSON.parse(hook("stop.sh", { session_id: "t-4" }).stdout).decision).toBe("block");
      hook("turn-start.sh", { session_id: "t-4" });
      put(repo, "app/q2.py", "def g():\n    return declarative_base(2)\n");
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
      const snap = join(stateDir(repo), "t-5.turn");
      expect(readFileSync(join(snap, "head"), "utf8").trim()).toBe(git(repo, "rev-parse", "HEAD").stdout.trim());
      expect(readFileSync(join(snap, "dirty"), "utf8")).toBe(`app/wip.py\t${git(repo, "hash-object", "app/wip.py").stdout.trim()}\n`);
      expect(readFileSync(join(snap, "files/app/wip.py"), "utf8")).toBe("X = 1\n");
      reset();
    },
    T,
  );

  /** @rule python/sqlalchemy/no-declarative-base */
  test(
    "Stop checks only what changed during the turn: the user's broken work in another file does not block a clean change",
    () => {
      put(repo, "app/wip.py", "def f():\n    return declarative_base(1)\n");
      hook("turn-start.sh", { session_id: "t-6" });
      put(repo, "app/ok.py", "X = 1\n");
      put(repo, "tests/test_ok.py", OK_TEST);
      expect(hook("stop.sh", { session_id: "t-6" }).stdout).toBe("");
      reset();
    },
    T,
  );

  /** @rule python/sqlalchemy/no-declarative-base */
  test(
    "in a file the user had already changed, only the lines added during the turn count",
    () => {
      put(repo, "app/wip.py", "def f():\n    return declarative_base(1)\n");
      hook("turn-start.sh", { session_id: "t-7" });
      put(repo, "app/wip.py", "def f():\n    return declarative_base(1)\n\n\nX = 1\n");
      put(repo, "tests/test_wip.py", OK_TEST);
      expect(hook("stop.sh", { session_id: "t-7" }).stdout).toBe("");
      hook("turn-start.sh", { session_id: "t-7" });
      put(repo, "app/wip.py", "def f():\n    return declarative_base(1)\n\n\ndef g():\n    return declarative_base(2)\n");
      const reason = JSON.parse(hook("stop.sh", { session_id: "t-7" }).stdout).reason;
      expect(reason).toContain("forge:python/sqlalchemy/no-declarative-base app/wip.py:6:12");
      expect(reason).not.toContain("app/wip.py:2:12");
      reset();
    },
    T,
  );

  /** @rule python/pytest/run */
  test(
    "the block reason puts the failure before the skipped and ok notes, and keeps at most 40 lines",
    () => {
      // fails only with the turn's change: at the start of the turn the test passed
      stubTool(
        "pytest",
        'i=0; while [ $i -lt 60 ]; do echo "tests/test_main.py:1: PytestWarning: noise $i"; i=$((i + 1)); done; grep -q turn-change app/main.py || exit 0; echo "FAILED tests/test_main.py::test_health - assert 500 == 200"; exit 1',
      );
      try {
        hook("turn-start.sh", { session_id: "t-12" });
        put(repo, "app/main.py", 'async def health() -> dict[str, str]:\n    return {"status": "ok"}  # turn-change\n');
        const reason: string = JSON.parse(hook("stop.sh", { session_id: "t-12" }).stdout).reason;
        const lines = reason.split("\n");
        expect(lines.length).toBeLessThanOrEqual(40);
        expect(lines.slice(0, 3)).toEqual([
          "forge:python/pytest/run failed: 1 failing test(s) passed at the start of the turn:",
          "  tests/test_main.py::test_health",
          "FAILED tests/test_main.py::test_health - assert 500 == 200",
        ]);
        expect(reason).toContain("PytestWarning: noise 0");
        expect(lines.at(-1)).toBe("forge:python/sqlalchemy/fk-index skipped: python not installed");
      } finally {
        rmSync(join(repo, ".venv/bin/pytest"));
        reset();
      }
    },
    T,
  );

  /**
   * In better-auth a Stop check took about 300 s, and the agent often answered
   * a block without changing anything: the same suite ran again on the same
   * tree (about 2,250 s in the experiment).
   */
  /** @rule python/pytest/run */
  test(
    "a tree that failed is not checked again: the same reason comes back at once, still counting toward the hand-over, until a file changes",
    () => {
      const runs = join(repo, ".venv", "runs");
      rmSync(runs, { force: true });
      // each Stop that runs the check runs pytest twice: now, and at the start of the turn
      stubTool("pytest", `echo run >> "${runs}"; grep -q turn-change app/main.py || exit 0; echo "FAILED tests/test_main.py::test_health - assert 500 == 200"; exit 1`);
      try {
        hook("turn-start.sh", { session_id: "t-13" });
        put(repo, "app/main.py", 'async def health() -> dict[str, str]:\n    return {"status": "ok"}  # turn-change\n');
        const first: string = JSON.parse(hook("stop.sh", { session_id: "t-13" }).stdout).reason;
        expect(first).toContain("FAILED tests/test_main.py::test_health");
        // given again without running anything: the stand-in counts its runs
        for (let i = 0; i < 2; i++) {
          const again = JSON.parse(hook("stop.sh", { session_id: "t-13" }).stdout);
          expect(again.reason).toBe(`forge: nothing changed in the working tree since this check failed, so it was not run again (it runs again once a file changes):\n${first}`);
        }
        expect(readFileSync(runs, "utf8")).toBe("run\nrun\n");
        expect(JSON.parse(hook("stop.sh", { session_id: "t-13" }).stdout).systemMessage).toContain("Handing over");
        put(repo, "app/main.py", 'async def health() -> dict[str, str]:\n    return {"status": "fine"}  # turn-change\n');
        // checked again: the same failure; the advice given with it the first time is accepted, not repeated (§10.7)
        // (notes come after the failures, sorted: §10.2 step 7)
        const isNote = (l: string) => /^forge:\S+ (skipped:|ok for |accepted:)/.test(l);
        const lines = first.replace(/forge:python\/pytest\/untested-change failed\n[^\n]*\n/, "").split("\n");
        const notes = [...lines.filter(isNote), "forge:python/pytest/untested-change accepted: reported once this turn"].sort();
        expect(JSON.parse(hook("stop.sh", { session_id: "t-13" }).stdout).reason).toBe([...lines.filter((l) => !isNote(l)), ...notes].join("\n"));
        expect(readFileSync(runs, "utf8")).toBe("run\nrun\nrun\nrun\n");
      } finally {
        rmSync(join(repo, ".venv/bin/pytest"));
        rmSync(runs, { force: true });
        reset();
      }
    },
    T,
  );

  /** @rule python/sqlalchemy/no-declarative-base */
  test(
    "a file whose name is not ASCII is part of the turn's change: Stop checks it like any other",
    () => {
      hook("turn-start.sh", { session_id: "t-utf8" });
      put(repo, "app/café.py", "def f():\n    return declarative_base(1)\n");
      const reason = JSON.parse(hook("stop.sh", { session_id: "t-utf8" }).stdout).reason;
      expect(reason).toContain("forge:python/sqlalchemy/no-declarative-base app/café.py:2:12");
      reset();
    },
    T,
  );

  /**
   * Claude Code stops the Stop hook after 600 s, and a stopped hook blocks nothing.
   * A test run (300 s), its re-run at the start of the turn (300 s) and the other
   * checks could exceed that, so they share one budget under it.
   * @rule python/pytest/run
   */
  test(
    "the checks of a Stop share one time budget under the hook's timeout: a check still running when it runs out is stopped and blocks, saying so",
    () => {
      stubTool("pytest", "sleep 60; exit 0");
      // A clock that stands still (date +%s; anything else is the real date): the checks
      // before pytest take no time from the budget however busy the machine is, so the
      // check still running when it runs out is pytest. What is tested is how the hooks
      // share the budget, not the clock; the stop itself is a real sleep of the budget.
      const clock = mkdtempSync(join(tmpdir(), "forge-clock-"));
      writeFileSync(join(clock, "date"), `#!/bin/sh\n[ "$*" = +%s ] || exec /bin/date "$@"\n[ -s "${clock}/now" ] || /bin/date +%s > "${clock}/now"\ncat "${clock}/now"\n`);
      chmodSync(join(clock, "date"), 0o755);
      try {
        hook("turn-start.sh", { session_id: "t-budget" });
        put(repo, "app/main.py", 'async def health() -> dict[str, str]:\n    return {"status": "ok"}  # turn-change\n');
        const t0 = performance.now();
        const out = JSON.parse(hook("stop.sh", { session_id: "t-budget" }, { FORGE_STOP_BUDGET: "6", PATH: `${clock}:${process.env.PATH}` }).stdout);
        expect(performance.now() - t0).toBeLessThan(30_000);
        expect(out.decision).toBe("block");
        expect(out.reason).toMatch(/forge:python\/pytest\/run failed: stopped after \d+s: the Stop hook's time budget \(6 s, FORGE_STOP_BUDGET\) ran out/);
      } finally {
        rmSync(join(repo, ".venv/bin/pytest"));
        reset();
      }
    },
    T,
  );

  /**
   * Advice (§10.7) comes from lint-stage checks, before the tests run, but a
   * failing test is what Claude must deal with first; and the failing tree is
   * still remembered, so stopping again on it runs nothing.
   * @rule python/pytest/run python/pytest/untested-change
   */
  test(
    "with advice and a failing test, the block shows the failure first and the advice after it; the failing tree is remembered and replayed without running the tests again, and the advice is not repeated once the tree changes",
    () => {
      const runs = join(repo, ".venv", "runs");
      rmSync(runs, { force: true });
      stubTool("pytest", `echo run >> "${runs}"; grep -q turn-change app/main.py || exit 0; echo "FAILED tests/test_main.py::test_health - assert 500 == 200"; exit 1`);
      try {
        hook("turn-start.sh", { session_id: "t-adv" });
        put(repo, "app/main.py", 'async def health() -> dict[str, str]:\n    return {"status": "ok"}  # turn-change\n');
        const first: string = JSON.parse(hook("stop.sh", { session_id: "t-adv" }).stdout).reason;
        const failure = first.indexOf("forge:python/pytest/run failed");
        const advice = first.indexOf("forge:python/pytest/untested-change failed");
        expect(failure).toBe(0);
        expect(advice).toBeGreaterThan(failure);
        expect(JSON.parse(hook("stop.sh", { session_id: "t-adv" }).stdout).reason).toBe(
          `forge: nothing changed in the working tree since this check failed, so it was not run again (it runs again once a file changes):\n${first}`,
        );
        expect(readFileSync(runs, "utf8")).toBe("run\nrun\n");
        put(repo, "app/main.py", 'async def health() -> dict[str, str]:\n    return {"status": "fine"}  # turn-change\n');
        const second: string = JSON.parse(hook("stop.sh", { session_id: "t-adv" }).stdout).reason;
        expect(second).toStartWith("forge:python/pytest/run failed");
        expect(second).not.toContain("untested-change failed");
      } finally {
        rmSync(join(repo, ".venv/bin/pytest"));
        rmSync(runs, { force: true });
        reset();
      }
    },
    T,
  );

  test(
    "a commit made during the turn is still part of the turn",
    () => {
      hook("turn-start.sh", { session_id: "t-8" });
      put(repo, "app/q.py", "def f():\n    return declarative_base(1)\n");
      git(repo, "add", "-A");
      git(repo, "commit", "-qm", "q", "--no-verify");
      expect(JSON.parse(hook("stop.sh", { session_id: "t-8" }).stdout).reason).toContain("forge:python/sqlalchemy/no-declarative-base app/q.py:2:12");
      git(repo, "reset", "-q", "--hard", "HEAD~1");
    },
    T,
  );

  /** @rule python/sqlalchemy/no-declarative-base */
  test(
    "post-edit reports findings only on lines added during the turn",
    () => {
      put(repo, "app/wip.py", "def f():\n    return declarative_base(1)\n");
      hook("turn-start.sh", { session_id: "t-9" });
      put(repo, "app/wip.py", "def f():\n    return declarative_base(1)\n\n\nX = 1\n");
      expect(hook("post-edit.sh", { session_id: "t-9", ...edit("app/wip.py") }).stdout).toBe("");
      put(repo, "app/wip.py", "def f():\n    return declarative_base(1)\n\n\ndef g():\n    return declarative_base(2)\n");
      const ctx = JSON.parse(hook("post-edit.sh", { session_id: "t-9", ...edit("app/wip.py") }).stdout).hookSpecificOutput.additionalContext;
      expect(ctx).toContain("app/wip.py:6:12");
      expect(ctx).not.toContain("app/wip.py:2:12");
      reset();
    },
    T,
  );

  /**
   * Formatting is whole-file: after an edit, `ruff format` may also rewrite lines
   * Claude never touched (here, two spaces before an inline comment). Those lines are
   * compared with the base copy formatted the same way (`format_stdin`), so they do
   * not count as added; without a formatter, or when it fails on the base, the plain
   * comparison applies.
   * @rule python/format python/no-stub
   */
  test(
    "a line only the formatter changed is not an added line: an untouched stub reformatted after an edit blocks neither post-edit nor Stop, while a new stub does",
    () => {
      const before = "class Span:\n    def set_attribute(self, key: str) -> None: ... # pragma: no cover\n\n    def end(self) -> int:\n        return 1\n";
      put(repo, "app/span.py", before);
      hook("turn-start.sh", { session_id: "t-fmt" });
      put(repo, "app/span.py", before.replace("return 1", "return 2"));
      addTest();
      expect(hook("post-edit.sh", { session_id: "t-fmt", ...edit("app/span.py") }).stdout).toBe("");
      expect(readFileSync(join(repo, "app/span.py"), "utf8")).toContain("...  # pragma: no cover");
      expect(hook("stop.sh", { session_id: "t-fmt" }).stdout).toBe("");
      put(repo, "app/span.py", readFileSync(join(repo, "app/span.py"), "utf8") + "\n    def flush(self) -> None: ...\n");
      const ctx = JSON.parse(hook("post-edit.sh", { session_id: "t-fmt", ...edit("app/span.py") }).stdout).hookSpecificOutput.additionalContext;
      expect(ctx).toContain("forge:python/no-stub app/span.py:7:");
      expect(ctx).not.toContain("app/span.py:2:");
      reset();
    },
    T,
  );

  /** @rule python/format python/no-stub */
  test(
    "when the formatter fails on the base copy (it does not parse), lines are compared with the base as it is",
    () => {
      put(repo, "app/span2.py", "class Span:\n    def set_attribute(self) -> None: ... # pragma: no cover\n\nX = (\n");
      hook("turn-start.sh", { session_id: "t-fmt2" });
      put(repo, "app/span2.py", "class Span:\n    def set_attribute(self) -> None: ... # pragma: no cover\n\nX = 1\n");
      const ctx = JSON.parse(hook("post-edit.sh", { session_id: "t-fmt2", ...edit("app/span2.py") }).stdout).hookSpecificOutput.additionalContext;
      expect(ctx).toContain("forge:python/no-stub app/span2.py:2:");
      reset();
    },
    T,
  );

  /**
   * The diff against the base and the one against the formatted base can align a
   * repeated line differently: Claude duplicating an existing line is added as the
   * first copy in one and as the second in the other. Such a line still counts.
   * @rule python/format python/no-placeholder-comment
   */
  test(
    "a line Claude duplicated in a file the formatter also rewrote is still an added line",
    () => {
      put(repo, "app/rate.py", "A  = 1\nRATE = 0.2  # placeholder value\n");
      hook("turn-start.sh", { session_id: "t-fmt3" });
      put(repo, "app/rate.py", "A = 1\nRATE = 0.2  # placeholder value\nRATE = 0.2  # placeholder value\n");
      addTest();
      const ctx = JSON.parse(hook("post-edit.sh", { session_id: "t-fmt3", ...edit("app/rate.py") }).stdout).hookSpecificOutput.additionalContext;
      expect(ctx).toMatch(/forge:python\/no-placeholder-comment app\/rate\.py:[23] /);
      expect(hook("stop.sh", { session_id: "t-fmt3" }).stdout).toContain("forge:python/no-placeholder-comment app/rate.py:");
      reset();
    },
    T,
  );

  /** @rule python/typecheck */
  test.if(hasUv)(
    "typecheck wakes Claude only for errors in files changed during the turn",
    () =>
      withRealTy(() => {
        put(repo, "app/wip.py", 'X: int = "no"\n');
        hook("turn-start.sh", { session_id: "t-10" });
        // the turn changes a file with no type error (the sample's main.py has a real one: fastapi is not installed here)
        put(repo, "app/clean.py", "Y: int = 1\n");
        const r = hook("typecheck.sh", { session_id: "t-10", ...edit("app/clean.py") });
        expect(r.status).toBe(0);
        expect(r.stderr).toBe("");
      }),
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

  /**
   * Advisory checks (§10.7) report something Claude may have a reason to leave as
   * it is: they block Stop once, and the next Stop of the same turn passes.
   */
  /** @rule python/pytest/untested-change */
  test(
    "an advisory check blocks Stop once per turn: the next Stop of the same turn passes, a new prompt re-arms it, pre-commit does not run it",
    () => {
      hook("turn-start.sh", { session_id: "t-14" });
      put(repo, "app/ok.py", "X = 1\n");
      const reason = JSON.parse(hook("stop.sh", { session_id: "t-14" }).stdout).reason;
      expect(reason).toContain("forge:python/pytest/untested-change failed\nYou changed behaviour in app/ok.py without adding or updating a test");
      expect(hook("stop.sh", { session_id: "t-14" }).stdout).toBe("");
      expect(sh("sh", [".forge/check.sh", "--changed"], { cwd: repo }).stdout).not.toContain("untested-change");
      hook("turn-start.sh", { session_id: "t-14" });
      put(repo, "app/ok.py", "X = 2\n");
      expect(JSON.parse(hook("stop.sh", { session_id: "t-14" }).stdout).reason).toContain("forge:python/pytest/untested-change failed");
      reset();
    },
    T,
  );

  /**
   * Advice is reported once per finding, not once per rule: what Claude already
   * answered is not repeated, but a later finding of the same rule in the turn is
   * given too.
   * @rule python/pytest/untested-change
   */
  test(
    "advice is reported once per finding: a Stop with nothing new passes, a new finding of the same rule later in the turn blocks again",
    () => {
      hook("turn-start.sh", { session_id: "t-once" });
      put(repo, "app/ok.py", "X = 1\n");
      expect(JSON.parse(hook("stop.sh", { session_id: "t-once" }).stdout).reason).toContain("You changed behaviour in app/ok.py without");
      put(repo, "app/ok.py", "X = 3\n");
      const same = hook("stop.sh", { session_id: "t-once" }).stdout;
      expect(same).toBe("");
      put(repo, "app/other.py", "Y = 1\n");
      expect(JSON.parse(hook("stop.sh", { session_id: "t-once" }).stdout).reason).toContain("You changed behaviour in app/ok.py, app/other.py without");
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
    stopEngines(ts);
  },
  T,
);

/**
 * In better-auth every Stop was blocked by tests that also failed before the
 * change: the container had no Postgres (`ECONNREFUSED 127.0.0.1:5432`) and no
 * network (`EAI_AGAIN`). These tests put a stand-in pytest in the sample's
 * `.venv` that logs where it ran, with which files, and whether it saw the
 * turn's change.
 */
/** @rule python/pytest/run */
describe("failing tests at the start of the turn", () => {
  const log = () => join(repo, ".venv", "pytest.log");
  const db = 'echo "E   ConnectionRefusedError: connect ECONNREFUSED 127.0.0.1:5432"; echo "FAILED tests/test_main.py::test_db - ConnectionRefusedError: connect ECONNREFUSED 127.0.0.1:5432"';
  function pytest(body: string) {
    rmSync(log(), { force: true });
    stubTool("pytest", `printf '%s|%s|%s\\n' "$(pwd)" "$*" "$(grep -c turn-change app/main.py)" >> "${log()}"; ${body}; exit 1`);
  }
  const change = () => put(repo, "app/main.py", 'async def health() -> dict[str, str]:\n    return {"status": "ok"}  # turn-change\n');
  const runs = () =>
    readFileSync(log(), "utf8")
      .trimEnd()
      .split("\n")
      .map((l) => l.split("|"));

  test(
    "only the failing test files are run again, in a copy of the tree as it was at the start of the turn; the working tree, index, stash and worktrees are left alone",
    () => {
      pytest(db);
      try {
        put(repo, "app/wip.py", "X = 1\n");
        git(repo, "add", "app/wip.py");
        hook("turn-start.sh", { session_id: "p-1" });
        change();
        const status = git(repo, "status", "--porcelain").stdout;
        hook("stop.sh", { session_id: "p-1" });
        const [now, base] = runs();
        expect(now).toEqual([realpathSync(repo), "-q tests/test_main.py", "1"]);
        expect(base![0]).not.toBe(realpathSync(repo));
        expect(base!.slice(1)).toEqual(["-q tests/test_main.py", "0"]);
        expect(git(repo, "status", "--porcelain").stdout).toBe(status);
        expect(readFileSync(join(repo, "app/main.py"), "utf8")).toContain("turn-change");
        expect(git(repo, "stash", "list").stdout).toBe("");
        expect(git(repo, "worktree", "list").stdout.trim().split("\n").length).toBe(1);
        expect(existsSync(base![0]!)).toBe(false);
      } finally {
        rmSync(join(repo, ".venv/bin/pytest"));
        reset();
      }
    },
    T,
  );

  test(
    "when every failing test also fails at the start of the turn, Stop blocks once naming them, their environmental errors and what to do; stopping again on the same tree is allowed, and the user is told what could not be verified",
    () => {
      pytest(db);
      try {
        hook("turn-start.sh", { session_id: "p-2" });
        change();
        const reason: string = JSON.parse(hook("stop.sh", { session_id: "p-2" }).stdout).reason;
        expect(reason).toStartWith(
          "forge:python/pytest/run 1 failing test(s) also fail at the start of the turn (re-run there), so this change did not cause them, but it is not verified by them either:\n  tests/test_main.py::test_db\nTheir errors look environmental:\n  Postgres refused the connection at 127.0.0.1:5432 (ECONNREFUSED)\n",
        );
        expect(reason).toContain("If that is not possible, stop again without changing files");
        const again = JSON.parse(hook("stop.sh", { session_id: "p-2" }).stdout);
        expect(again.decision).toBeUndefined();
        expect(again.systemMessage).toBe(
          "forge: 1 test(s) could not be verified: they fail with and without Claude's change, and their errors look environmental: Postgres refused the connection at 127.0.0.1:5432 (ECONNREFUSED). Not verified: python/pytest/run tests/test_main.py::test_db. Run them once the environment is set up.",
        );
        expect(runs().length).toBe(2);
        expect(hook("stop.sh", { session_id: "p-2" }).stdout).toBe("");
      } finally {
        rmSync(join(repo, ".venv/bin/pytest"));
        reset();
      }
    },
    T,
  );

  test(
    "a test file whose name is not ASCII is in the copy of the tree at the start of the turn",
    () => {
      const head = git(repo, "rev-parse", "HEAD").stdout.trim();
      put(repo, "tests/test_café.py", "def test_db() -> None:\n    assert True\n");
      git(repo, "add", "-A");
      git(repo, "commit", "-qm", "café", "--no-verify");
      pytest('[ -f tests/test_café.py ] && echo "FAILED tests/test_café.py::test_db - ConnectionRefusedError: connect ECONNREFUSED 127.0.0.1:5432"');
      try {
        hook("turn-start.sh", { session_id: "p-utf8" });
        change();
        const reason: string = JSON.parse(hook("stop.sh", { session_id: "p-utf8" }).stdout).reason;
        expect(reason).toStartWith("forge:python/pytest/run 1 failing test(s) also fail at the start of the turn (re-run there)");
        expect(reason).toContain("  tests/test_café.py::test_db\n");
      } finally {
        rmSync(join(repo, ".venv/bin/pytest"));
        git(repo, "reset", "-q", "--hard", head);
        reset();
      }
    },
    T,
  );

  test(
    "after the comparison, the output excerpt is at most 20 lines under new failures and 12 under failures that were already there",
    () => {
      const noise = 'i=0; while [ $i -lt 60 ]; do echo "noise $i"; i=$((i + 1)); done';
      const excerpt = (reason: string) => {
        const lines = reason.split("\n");
        const from = lines.findIndex((l) => l === "noise 0" || l.startsWith("FAILED "));
        const to = lines.findIndex((l, i) => i > from && l.startsWith("... ("));
        return lines.slice(from, to + 1);
      };
      pytest(`${noise}; if grep -q turn-change app/main.py; then echo "FAILED tests/test_main.py::test_health - assert 500 == 200"; fi`);
      try {
        hook("turn-start.sh", { session_id: "p-x1" });
        change();
        const fresh = excerpt(JSON.parse(hook("stop.sh", { session_id: "p-x1" }).stdout).reason);
        expect(fresh[0]).toBe("FAILED tests/test_main.py::test_health - assert 500 == 200");
        expect(fresh.length).toBe(20);
        reset();
        pytest(`${noise}; ${db}`);
        hook("turn-start.sh", { session_id: "p-x2" });
        change();
        const old = excerpt(JSON.parse(hook("stop.sh", { session_id: "p-x2" }).stdout).reason);
        expect(old[0]).toStartWith("FAILED tests/test_main.py::test_db");
        expect(old.length).toBe(12);
      } finally {
        rmSync(join(repo, ".venv/bin/pytest"));
        reset();
      }
    },
    T,
  );

  /**
   * The copy shares the dependencies of the working tree, but not the project's
   * own code: an editable install (uv, pip -e, Poetry) puts the working tree's
   * src/ on Python's path through a .pth file, and the copy's code must come first.
   */
  function editableRepo(install: (site: string, root: string) => void, pkg = "mypkg"): string {
    const r = applied("fastapi-app");
    put(r, ".gitignore", `${readFileSync(join(r, ".gitignore"), "utf8")}local.db\n`);
    put(r, `src/${pkg}/__init__.py`, "");
    put(r, `src/${pkg}/f.py`, "def f() -> int:\n    return 1\n");
    put(r, "tests/test_f.py", "from mypkg.f import f\n\n\ndef test_f() -> None:\n    assert f() == 1\n");
    git(r, "add", "-A");
    git(r, "commit", "-qm", "pkg", "--no-verify");
    put(r, "local.db", "seed\n");
    expect(sh("python3", ["-m", "venv", "--without-pip", join(r, ".venv")]).status).toBe(0);
    const lib = join(r, ".venv", "lib");
    install(join(lib, readdirSync(lib).find((d) => d.startsWith("python"))!, "site-packages"), realpathSync(r));
    // a stand-in pytest: imports the package with the venv's python, and writes to an ignored file
    writeFileSync(
      join(r, ".venv", "bin", "pytest"),
      `#!/bin/sh\nprintf 'x\\n' >> local.db\nexec "$(dirname "$0")/python" -c 'import sys\nfrom mypkg.f import f\nif f() != 1:\n    print("FAILED tests/test_f.py::test_f - assert", f(), "== 1")\n    sys.exit(1)'\n`,
    );
    chmodSync(join(r, ".venv", "bin", "pytest"), 0o755);
    return r;
  }
  const stopIn = (r: string, session: string, change: () => void) => {
    hook("turn-start.sh", { session_id: session }, {}, r);
    change();
    return JSON.parse(hook("stop.sh", { session_id: session }, {}, r).stdout).reason as string;
  };

  test(
    "with an editable install of the project in its virtual environment, the copy imports its own code, not the working tree's: a regression is not excused as already failing; the user's ignored files are copied, not written",
    () => {
      const r = editableRepo((site, root) => writeFileSync(join(site, "_editable_impl_mypkg.pth"), `${root}/src\n`));
      const reason = stopIn(r, "e-1", () => put(r, "src/mypkg/f.py", "def f() -> int:\n    return 2\n"));
      expect(reason).toContain("forge:python/pytest/run failed: 1 failing test(s) passed at the start of the turn:\n  tests/test_f.py::test_f\n");
      // pytest ran twice (now, and in the copy); only the run in the working tree wrote there
      expect(readFileSync(join(r, "local.db"), "utf8")).toBe("seed\nx\n");
    },
    T,
  );

  test(
    "when the copy cannot be isolated (an editable-install finder maps the package to the working tree), the failing tests count as new and the block says the comparison was not possible",
    () => {
      const r = editableRepo((site, root) => {
        writeFileSync(
          join(site, "__editable___mypkg_finder.py"),
          `import importlib.util, sys\nMAPPING = {"mypkg": "${root}/src/impl"}\nclass _F:\n    @classmethod\n    def find_spec(cls, name, path=None, target=None):\n        if name in MAPPING:\n            return importlib.util.spec_from_file_location(name, MAPPING[name] + "/__init__.py", submodule_search_locations=[MAPPING[name]])\n        return None\ndef install():\n    sys.meta_path.append(_F)\n`,
        );
        writeFileSync(join(site, "__editable__.mypkg-0.1.0.pth"), "import __editable___mypkg_finder; __editable___mypkg_finder.install()\n");
      }, "impl");
      const reason = stopIn(r, "e-2", () => put(r, "src/impl/f.py", "def f() -> int:\n    return 2\n"));
      expect(reason).toContain(
        "forge:python/pytest/run failed: 1 failing test(s); comparison with the start of the turn was not possible (.venv/bin/python imports mypkg from the working tree, not from the copy), so they count as new:\n  tests/test_f.py::test_f\n",
      );
    },
    T,
  );

  test(
    "a test that passed at the start of the turn and fails now blocks as usual, named first; the ones failing before too are counted, not blamed",
    () => {
      pytest(`${db}; if grep -q turn-change app/main.py; then echo "FAILED tests/test_main.py::test_health - assert 500 == 200"; fi`);
      try {
        hook("turn-start.sh", { session_id: "p-3" });
        change();
        const reason: string = JSON.parse(hook("stop.sh", { session_id: "p-3" }).stdout).reason;
        expect(reason).toStartWith(
          "forge:python/pytest/run failed: 1 failing test(s) passed at the start of the turn:\n  tests/test_main.py::test_health\n(1 other failing test(s) also fail at the start of the turn: not caused by this change)\n",
        );
        expect(JSON.parse(hook("stop.sh", { session_id: "p-3" }).stdout).reason).toStartWith("forge: nothing changed in the working tree");
      } finally {
        rmSync(join(repo, ".venv/bin/pytest"));
        reset();
      }
    },
    T,
  );
});

/**
 * Errors that were already in files Claude did not touch are not Claude's: the
 * Stop hook and the background type check report only errors in changed files.
 */
describe("errors in unchanged files", () => {
  function legacyRepo(): string {
    return fixture("eslint-app-legacy", (r) => {
      ensureTools();
      copyRepo(sampleRepo("eslint-app"), r);
      linkTools(r, "node");
      put(r, "src/legacy.ts", 'export const legacy: number = "oops";\n');
      commitAll(r, "legacy");
      expect(forge(["apply", "--yes"], r).status).toBe(0);
      commitAll(r, "forge");
    });
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
