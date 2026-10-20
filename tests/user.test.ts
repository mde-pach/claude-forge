// §7: user level, with a fake HOME.
import { expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { parse } from "jsonc-parser/lib/esm/main.js";
import { fakeClaude, forge, hookJson, sh } from "./helpers";

const T = 60_000;

function home(settings?: string): { home: string; env: Record<string, string> } {
  const h = mkdtempSync(join(tmpdir(), "forge-home-"));
  if (settings !== undefined) {
    mkdirSync(join(h, ".claude"), { recursive: true });
    writeFileSync(join(h, ".claude", "settings.json"), settings);
  }
  return { home: h, env: { ...fakeClaude(), HOME: h } };
}

test(
  "fresh HOME: files, settings, idempotent, remove leaves nothing",
  () => {
    const { home: h, env } = home();
    const r = forge(["apply", "--user", "--yes"], "/", env);
    expect(r.status).toBe(0);
    const root = join(h, ".claude/skills/forge-user");
    for (const f of [".claude-plugin/plugin.json", "hooks/hooks.json", "scripts/guard.sh", "scripts/guard.rules", "scripts/notify.sh", "scripts/lib.sh"])
      expect(existsSync(join(root, f))).toBe(true);
    const settings = JSON.parse(readFileSync(join(h, ".claude/settings.json"), "utf8"));
    expect(settings.permissions.deny).toContain("Bash(git reset --hard:*)");
    expect(settings.permissions.deny).toContain("Read(~/.ssh/**)");
    expect(settings.permissions.ask).toEqual(["Edit(~/.claude/skills/forge-user/**)", "Edit(~/.claude/forge.lock.json)"]);
    expect(settings.sandbox).toEqual({ enabled: true, network: { allowedDomains: ["registry.npmjs.org", "pypi.org", "files.pythonhosted.org", "github.com"] } });
    const hooks = JSON.parse(readFileSync(join(root, "hooks/hooks.json"), "utf8")).hooks;
    expect(hooks.PreToolUse[0]).toMatchObject({ matcher: "Bash", hooks: [{ timeout: 5, onFailure: "block" }] });
    expect(hooks.Notification[0].matcher).toBe("permission_prompt|idle_prompt");

    expect(forge(["apply", "--user", "--yes"], "/", env).stdout).toContain("up to date");
    expect(forge(["diff", "--user"], "/", env).status).toBe(0);

    // the user-level guard blocks the safety corpus
    const g = sh("sh", [join(root, "scripts/guard.sh")], { input: hookJson({ tool_name: "Bash", tool_input: { command: "git push --force origin main" } }) });
    expect(JSON.parse(g.stdout).hookSpecificOutput.permissionDecision).toBe("deny");

    expect(forge(["remove", "--user", "--yes"], "/", env).status).toBe(0);
    expect(sh("find", [h, "-type", "f"]).stdout).toBe("");
  },
  T,
);

test(
  "existing settings: comments kept, user entries untouched, scalar conflicts warned",
  () => {
    const original = '{\n  // mine\n  "model": "opus",\n  "permissions": {"deny": ["Read(**/.env)"]},\n  "sandbox": {"enabled": false}\n}\n';
    const { home: h, env } = home(original);
    const r = forge(["apply", "--user", "--yes"], "/", env);
    expect(r.status).toBe(0);
    expect(r.stderr).toContain("sandbox.enabled is false; forge wants true (left unchanged)");
    const text = readFileSync(join(h, ".claude/settings.json"), "utf8");
    expect(text).toContain("// mine");
    const s = parse(text);
    expect(s.sandbox.enabled).toBe(false);
    expect(s.permissions.deny.filter((x: string) => x === "Read(**/.env)").length).toBe(1);
    const lock = JSON.parse(readFileSync(join(h, ".claude/forge.lock.json"), "utf8"));
    expect(lock.settings.some((e: { value: unknown }) => e.value === "Read(**/.env)")).toBe(false);
    expect(lock.settings.some((e: { path: string }) => e.path === "sandbox.enabled")).toBe(false);
    expect(forge(["status", "--user"], "/", env).stdout).toContain("sandbox.enabled is false");

    expect(forge(["remove", "--user", "--yes"], "/", env).status).toBe(0);
    const back = readFileSync(join(h, ".claude/settings.json"), "utf8");
    expect(back).toContain("// mine");
    expect(parse(back)).toEqual(parse(original));
  },
  T,
);

test("notify.sh: OSC 777 by default, OSC 9 for iTerm.app and WezTerm", () => {
  const { env } = home();
  expect(forge(["apply", "--user", "--yes"], "/", env).status).toBe(0);
  const script = join(env.HOME!, ".claude/skills/forge-user/scripts/notify.sh");
  const run = (term?: string) =>
    JSON.parse(sh("sh", [script], { input: hookJson({ message: "Claude needs \u0007 your permission" }), env: { TERM_PROGRAM: term ?? "" } }).stdout).terminalSequence;
  expect(run()).toBe("\u001b]777;notify;Claude Code;Claude needs  your permission\u0007");
  expect(run("iTerm.app")).toBe("\u001b]9;Claude needs  your permission\u0007");
  expect(run("WezTerm")).toBe("\u001b]9;Claude needs  your permission\u0007");
});
