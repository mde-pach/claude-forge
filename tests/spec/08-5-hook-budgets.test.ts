/**
 * @spec 8.5 · Hook latency budgets
 *
 * Every hook runs on Claude's critical path, so each has a budget, measured warm (the
 * tools' facts cached, the types engine running, a turn snapshot taken) on the sample
 * FastAPI repository after an edit of one file: the commands it starts by name (git and
 * the shell utilities, counted through a PATH of logging stand-ins; the tools it runs by
 * path, ruff from .venv and ast-grep from uv's cache, are their own) and its median time
 * over five runs. The budgets hold on a slow machine too: a process costs about 25 ms
 * under emulation, so the counts are what keeps a hook fast everywhere.
 */
import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, statSync, symlinkSync, utimesSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { delimiter, join } from "node:path";
import { applied, commitAll, hasUv, hookJson, othersRunning, put, sh, stopEngines } from "../helpers";

const T = 300_000;
const SCRIPTS = ".claude/skills/forge-project/scripts";
const ruff = sh("sh", ["-c", "command -v ruff"]).stdout.trim();
let repo = "";

/** Each hook, its input (after an edit of app/main.py), its process budget and its latency budget (ms). */
const HOOKS: { script: string; fields: (repo: string) => Record<string, unknown>; processes: number; ms: number }[] = [
  { script: "guard.sh", fields: () => ({ hook_event_name: "PreToolUse", tool_name: "Bash", tool_input: { command: "ls" } }), processes: 3, ms: 100 },
  { script: "bash-snapshot.sh", fields: () => ({ hook_event_name: "PreToolUse", tool_name: "Bash", tool_use_id: "toolu_b", tool_input: { command: "ls" } }), processes: 9, ms: 250 },
  { script: "test-edit.sh", fields: (r) => ({ hook_event_name: "PreToolUse", tool_name: "Edit", tool_input: { file_path: join(r, "app/main.py") } }), processes: 3, ms: 150 },
  { script: "post-edit.sh", fields: (r) => ({ hook_event_name: "PostToolUse", tool_name: "Edit", tool_input: { file_path: join(r, "app/main.py") } }), processes: 18, ms: 600 },
  { script: "typecheck.sh", fields: (r) => ({ hook_event_name: "PostToolUse", tool_name: "Edit", tool_input: { file_path: join(r, "app/main.py") } }), processes: 16, ms: 400 },
  { script: "turn-start.sh", fields: () => ({ hook_event_name: "UserPromptSubmit", prompt: "go" }), processes: 18, ms: 400 },
  { script: "session-start.sh", fields: () => ({ hook_event_name: "SessionStart", source: "startup" }), processes: 10, ms: 500 },
];

function hook(script: string, fields: Record<string, unknown>, env: Record<string, string> = {}) {
  // the edit-time typecheck asks the types engine, as in a session (helpers.ts turns it off for the other specs)
  return sh("sh", [join(SCRIPTS, script)], {
    cwd: repo,
    input: hookJson({ cwd: repo, session_id: "budget", ...fields }),
    env: { CLAUDE_CODE_ENTRYPOINT: "cli", FORGE_EDIT_TYPECHECK_ENGINE: "auto", ...env },
  });
}

/**
 * A PATH of stand-ins, one per command of the current PATH, each logging its name to
 * $FORGE_COUNT_LOG and running the real one: what a hook starts by name, counted.
 */
function countingPath(): string {
  const dir = mkdtempSync(join(tmpdir(), "forge-counting-path-"));
  const seen = new Set<string>();
  for (const d of (process.env.PATH ?? "").split(delimiter)) {
    let names: string[] = [];
    try {
      names = readdirSync(d);
    } catch {
      continue;
    }
    for (const name of names) {
      if (seen.has(name) || !/^[A-Za-z0-9._+-]+$/.test(name)) continue;
      const real = join(d, name);
      try {
        const st = statSync(real);
        if (!st.isFile() || !(st.mode & 0o111)) continue;
      } catch {
        continue;
      }
      seen.add(name);
      writeFileSync(join(dir, name), `#!/bin/sh\nprintf '%s\\n' ${name} >> "$FORGE_COUNT_LOG"\nexec "${real}" "$@"\n`);
      chmodSync(join(dir, name), 0o755);
    }
  }
  // older than the facts the warm-up runs record: a fact is kept only when newer than what it watches (a tool's file and directory)
  const past = new Date(Date.now() - 60_000);
  for (const name of seen) utimesSync(join(dir, name), past, past);
  utimesSync(dir, past, past);
  return dir;
}

/**
 * The commands one run of <script> starts by name, after two warm-up runs with the same
 * PATH (the tools' facts are keyed by where a tool is found).
 */
function processes(script: string, fields: Record<string, unknown>, path: string): string[] {
  const log = join(mkdtempSync(join(tmpdir(), "forge-counting-")), "log");
  const env = { PATH: path, FORGE_COUNT_LOG: log };
  hook(script, fields, env);
  hook(script, fields, env);
  // what a run left in the background (SessionStart's engine check) ends first
  settle(log);
  writeFileSync(log, "");
  hook(script, fields, env);
  settle(log);
  return readFileSync(log, "utf8").split("\n").filter(Boolean);
}

/** Waits until nothing has been logged to <log> for 300 ms (at most 15 s). */
function settle(log: string): void {
  let size = -1;
  for (let i = 0; i < 50 && statSync(log).size !== size; i++) {
    size = statSync(log).size;
    Bun.sleepSync(300);
  }
}

/** The median of five warm runs of <script>, in ms. */
function median(script: string, fields: Record<string, unknown>): number {
  hook(script, fields);
  hook(script, fields);
  const times: number[] = [];
  for (let i = 0; i < 5; i++) {
    const t = performance.now();
    hook(script, fields);
    times.push(performance.now() - t);
  }
  return times.sort((a, b) => a - b)[2]!;
}

describe.skipIf(!hasUv || !ruff)("each hook within its budget, warm, after an edit", () => {
  beforeAll(() => {
    repo = applied("fastapi-app");
    commitAll(repo, "forge");
    mkdirSync(join(repo, ".venv", "bin"), { recursive: true });
    if (!existsSync(join(repo, ".venv", "bin", "ruff"))) symlinkSync(ruff, join(repo, ".venv", "bin", "ruff"));
    // warm: the session (types engine), a turn, an edit
    hook("session-start.sh", { hook_event_name: "SessionStart", source: "startup" });
    hook("turn-start.sh", { hook_event_name: "UserPromptSubmit", prompt: "go" });
    put(repo, "app/main.py", `${readFileSync(join(repo, "app/main.py"), "utf8")}\n\ndef budget_added() -> int:\n    return 1\n`);
  }, T);
  // the engine SessionStart started does not outlive the file
  afterAll(() => stopEngines(repo));

  test(
    "the commands each hook starts by name stay within its budget: guard 3, bash-snapshot 9, test-edit 3, post-edit 18, typecheck 16, turn-start 18, session-start 10",
    () => {
      const over: string[] = [];
      const path = countingPath();
      for (const h of HOOKS) {
        const p = processes(h.script, h.fields(repo), path);
        console.log(`${h.script}: ${p.length} commands (${p.join(" ")})`);
        if (p.length > h.processes) over.push(`${h.script}: ${p.length} > ${h.processes} (${p.join(" ")})`);
      }
      expect(over).toEqual([]);
    },
    T,
  );

  test.skipIf(othersRunning(import.meta.path))(
    "each hook's median time stays within its budget: guard 100 ms, bash-snapshot 250 ms, test-edit 150 ms, post-edit 600 ms, typecheck 400 ms, turn-start 400 ms, session-start 500 ms",
    () => {
      const over: string[] = [];
      for (const h of HOOKS) {
        const ms = median(h.script, h.fields(repo));
        console.log(`${h.script}: median ${Math.round(ms)} ms`);
        if (ms > h.ms) over.push(`${h.script}: ${Math.round(ms)} ms > ${h.ms} ms`);
      }
      expect(over).toEqual([]);
    },
    T,
  );
});
