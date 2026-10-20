/**
 * @spec 7 · User level
 *
 * `forge apply --user` writes a `forge-user` plugin under `~/.claude/skills/`
 * (a Bash guard and terminal notifications) and appends the `safety` pack's
 * permissions and sandbox settings to `~/.claude/settings.json`, plus `ask` on
 * Forge's own files. These tests run with a temporary `HOME`.
 */
import { expect, test } from "bun:test";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { parse } from "jsonc-parser/lib/esm/main.js";
import { fakeClaude, forge, hookJson, sh } from "../helpers";

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
  "apply --user writes the forge-user plugin: plugin.json, hooks.json, guard.sh, guard.rules, notify.sh, lib.sh",
  () => {
    const { home: h, env } = home();
    expect(forge(["apply", "--user", "--yes"], "/", env).status).toBe(0);
    const root = join(h, ".claude/skills/forge-user");
    for (const f of [".claude-plugin/plugin.json", "hooks/hooks.json", "scripts/guard.sh", "scripts/guard.rules", "scripts/notify.sh", "scripts/lib.sh"])
      expect(existsSync(join(root, f))).toBe(true);
    expect(JSON.parse(readFileSync(join(root, ".claude-plugin/plugin.json"), "utf8")).name).toBe("forge-user");
    const hooks = JSON.parse(readFileSync(join(root, "hooks/hooks.json"), "utf8")).hooks;
    expect(hooks.PreToolUse[0]).toMatchObject({ matcher: "Bash", hooks: [{ command: 'sh "${CLAUDE_PLUGIN_ROOT}/scripts/guard.sh"', timeout: 5, onFailure: "block" }] });
    expect(hooks.Notification[0].matcher).toBe("permission_prompt|idle_prompt");
  },
  T,
);

/** @pack safety */
test(
  "apply --user appends the safety pack's permissions and sandbox, and ask on Forge's own files",
  () => {
    const { home: h, env } = home();
    expect(forge(["apply", "--user", "--yes"], "/", env).status).toBe(0);
    const s = JSON.parse(readFileSync(join(h, ".claude/settings.json"), "utf8"));
    expect(s.permissions.deny).toEqual(expect.arrayContaining(["Bash(git reset --hard:*)", "Read(~/.ssh/**)", "Edit(**/.env)", "Bash(npm publish:*)"]));
    expect(s.permissions.ask).toEqual(["Edit(~/.claude/skills/forge-user/**)", "Edit(~/.claude/forge.lock.json)"]);
    expect(s.sandbox).toEqual({ enabled: true, network: { allowedDomains: ["registry.npmjs.org", "pypi.org", "files.pythonhosted.org", "github.com"] } });
  },
  T,
);

/** @rule safety/git-force-push */
test(
  "the user-level guard blocks a force push in any directory",
  () => {
    const { home: h, env } = home();
    expect(forge(["apply", "--user", "--yes"], "/", env).status).toBe(0);
    const g = sh("sh", [join(h, ".claude/skills/forge-user/scripts/guard.sh")], { input: hookJson({ tool_name: "Bash", tool_input: { command: "git push --force origin main" } }) });
    expect(JSON.parse(g.stdout).hookSpecificOutput).toMatchObject({ permissionDecision: "deny", permissionDecisionReason: expect.stringMatching(/^forge:safety\/git-force-push /) });
  },
  T,
);

/** @rule safety/sandbox */
test(
  "existing user settings keep their comments and values; a scalar set differently is left alone with a warning",
  () => {
    const original = '{\n  // mine\n  "model": "opus",\n  "permissions": {"deny": ["Read(**/.env)"]},\n  "sandbox": {"enabled": false}\n}\n';
    const { home: h, env } = home(original);
    const r = forge(["apply", "--user", "--yes"], "/", env);
    expect(r.status).toBe(0);
    expect(r.stderr).toContain("sandbox.enabled is false; forge wants true (left unchanged)");
    const s = parse(readFileSync(join(h, ".claude/settings.json"), "utf8"));
    expect(s.sandbox.enabled).toBe(false);
    expect(s.permissions.deny.filter((x: string) => x === "Read(**/.env)").length).toBe(1);
    expect(forge(["status", "--user"], "/", env).stdout).toContain("sandbox.enabled is false");
  },
  T,
);

test(
  "apply --user is idempotent, and remove --user leaves the user's settings as they were",
  () => {
    const original = '{\n  // mine\n  "model": "opus"\n}\n';
    const { home: h, env } = home(original);
    expect(forge(["apply", "--user", "--yes"], "/", env).status).toBe(0);
    expect(forge(["apply", "--user", "--yes"], "/", env).stdout).toContain("up to date");
    expect(forge(["diff", "--user"], "/", env).status).toBe(0);
    expect(forge(["remove", "--user", "--yes"], "/", env).status).toBe(0);
    expect(parse(readFileSync(join(h, ".claude/settings.json"), "utf8"))).toEqual(parse(original));
    expect(sh("find", [join(h, ".claude"), "-type", "f"]).stdout.trim()).toBe(join(h, ".claude/settings.json"));
  },
  T,
);

test.each([
  ["", "\u001b]777;notify;Claude Code;Claude needs  your permission\u0007"],
  ["iTerm.app", "\u001b]9;Claude needs  your permission\u0007"],
  ["WezTerm", "\u001b]9;Claude needs  your permission\u0007"],
])(
  "notify.sh with TERM_PROGRAM=%p prints the terminal sequence %j (control characters removed)",
  (term, sequence) => {
    const { env } = home();
    expect(forge(["apply", "--user", "--yes"], "/", env).status).toBe(0);
    const script = join(env.HOME!, ".claude/skills/forge-user/scripts/notify.sh");
    const out = sh("sh", [script], { input: hookJson({ message: "Claude needs \u0007 your permission" }), env: { TERM_PROGRAM: term } });
    expect(JSON.parse(out.stdout).terminalSequence).toBe(sequence);
  },
  T,
);
