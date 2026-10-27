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
import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { renderGuardRules, renderLib, template } from "../../src/render";
import { availableShells, guardDecision, guardExampleDir, hookInput } from "../../src/selftest";
import type { Rule } from "../../src/types";
import { sh } from "../helpers";

const shells = availableShells();
const lib = mkdtempSync(join(tmpdir(), "forge-lib-"));
writeFileSync(join(lib, "lib.sh"), renderLib());

/** Runs `snippet` after sourcing lib.sh, with $FORGE_INPUT set to `input`. */
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
    expect(libRun([...argv], "{}", "tool ruff; printf '|'; tool pytest", uvRepo)).toBe(".venv/bin/ruff|");
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
