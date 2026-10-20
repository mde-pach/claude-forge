import { existsSync, mkdirSync, mkdtempSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { globMatch, loadPacks, loadLocalRules } from "./packs.js";
import {
  astGrepRulePath,
  astGrepTestPath,
  renderAstGrepRule,
  renderAstGrepTest,
  renderGuardRules,
  renderSgConfig,
  template,
} from "./render.js";
import { AST_GREP_VERSION, run } from "./util.js";
import type { AstGrepTarget, GuardTarget, PermissionTarget, Rule } from "./types.js";

export interface TestReport {
  failures: string[];
  notes: string[];
  counts: Record<string, number>;
}

// ---------------------------------------------------------------- ast-grep

/** argv prefix that runs the pinned ast-grep, or undefined. */
export function astGrepCommand(): string[] | undefined {
  if (run("bunx", ["--version"]).status === 0) return ["bunx", "-p", `@ast-grep/cli@${AST_GREP_VERSION}`, "ast-grep"];
  if (run("uvx", ["--version"]).status === 0) return ["uvx", "-q", "--from", `ast-grep-cli==${AST_GREP_VERSION}`, "ast-grep"];
  return undefined;
}

// ---------------------------------------------------------------- guard

export function hookInput(command: string): string {
  return JSON.stringify({
    session_id: "forge-test",
    cwd: "/",
    hook_event_name: "PreToolUse",
    tool_name: "Bash",
    tool_input: { command, description: "test" },
  });
}

/** Runs guard.sh from <dir> under <shell> (argv) on <command>; returns "deny" | "ask" | "" (or "error: …"). */
export function guardDecision(shell: string[], dir: string, command: string): string {
  const r = run(shell[0]!, [...shell.slice(1), join(dir, "guard.sh")], { input: hookInput(command) });
  if (r.status !== 0) return `error: exit ${r.status} ${r.stderr.trim()}`;
  if (!r.stdout.trim()) return "";
  try {
    const out = JSON.parse(r.stdout) as { hookSpecificOutput?: { permissionDecision?: string } };
    return out.hookSpecificOutput?.permissionDecision ?? `error: ${r.stdout.trim()}`;
  } catch {
    return `error: not JSON: ${r.stdout.trim()}`;
  }
}

/** sh, dash, bash (POSIX mode, stands in for macOS bash 3.2) and BusyBox, as available. */
export function availableShells(): { name: string; argv: string[] }[] {
  const all = [
    { name: "sh", argv: ["sh"] },
    { name: "dash", argv: ["dash"] },
    { name: "bash --posix", argv: ["bash", "--posix"] },
    { name: "busybox sh", argv: ["busybox", "sh"] },
  ];
  return all.filter((s) => run(s.argv[0]!, [...s.argv.slice(1), "-c", "exit 0"]).status === 0);
}

// ---------------------------------------------------------------- permissions

/** Claude Code permission matching, reduced to what packs use. */
export function permissionMatches(rule: string, call: string): boolean {
  const pr = /^([A-Za-z]+)(?:\((.*)\))?$/.exec(rule);
  const pc = /^([A-Za-z]+)\((.*)\)$/.exec(call);
  if (!pr || !pc || pr[1] !== pc[1]) return false;
  const spec = pr[2];
  const arg = pc[2]!;
  if (spec === undefined) return true;
  if (pr[1] === "Bash") {
    if (spec.endsWith(":*")) {
      const prefix = spec.slice(0, -2);
      return arg === prefix || (arg.startsWith(prefix) && !/[A-Za-z0-9_]/.test(arg[prefix.length]!));
    }
    return arg === spec;
  }
  return globMatch(spec, arg);
}

// ---------------------------------------------------------------- forge test

export function selfTest(packsDir: string, repo?: string): TestReport {
  const failures: string[] = [];
  const notes: string[] = [];
  const counts: Record<string, number> = { packs: 0, rules: 0, astgrep: 0, guard: 0, permission: 0, context: 0 };

  let rules: Rule[] = [];
  try {
    const packs = loadPacks(packsDir);
    counts.packs = packs.size;
    for (const p of packs.values()) {
      rules.push(...p.rules);
      if (p.context !== undefined) {
        counts.context!++;
        const lines = p.context.replace(/\n$/, "").split("\n").length;
        if (lines > 10) failures.push(`${p.id}/context.md: ${lines} lines (max 10)`);
      }
    }
    if (repo) {
      const local = loadLocalRules(repo);
      const ids = new Map(rules.map((r, i) => [r.id, i]));
      for (const r of local) {
        const i = ids.get(r.id);
        if (i !== undefined) rules[i] = r;
        else rules.push(r);
      }
      if (local.length) notes.push(`including ${local.length} local rule(s) from .forge/rules`);
    }
  } catch (e) {
    failures.push((e as Error).message);
    return { failures, notes, counts };
  }
  counts.rules = rules.length;

  // structural checks the schema cannot express
  for (const r of rules) {
    const ast = r.enforce.filter((t) => t.target === "ast-grep");
    if (ast.length > 1) failures.push(`${r.id}: at most one ast-grep target`);
    if (ast.length && r.scope.language === "any") failures.push(`${r.id}: ast-grep targets need a concrete scope.language`);
    const needsExamples = r.enforce.some((t) => ["ast-grep", "guard", "permission", "tool"].includes(t.target));
    if (needsExamples && !(r.examples?.match?.length && r.examples?.no_match?.length))
      failures.push(`${r.id}: needs match and no_match examples`);
  }

  // ast-grep rule-tests
  const astRules = rules.filter((r) => r.enforce.some((t) => t.target === "ast-grep"));
  counts.astgrep = astRules.length;
  if (astRules.length) {
    const cmd = astGrepCommand();
    if (!cmd) failures.push("ast-grep: neither bunx nor uvx is available");
    else {
      const dir = mkdtempSync(join(tmpdir(), "forge-test-"));
      try {
        writeFileSync(join(dir, "sgconfig.yml"), renderSgConfig());
        for (const r of astRules) {
          const t = r.enforce.find((x): x is AstGrepTarget => x.target === "ast-grep")!;
          for (const [rel, text] of [
            [astGrepRulePath(r.id), renderAstGrepRule(r, t)],
            [astGrepTestPath(r.id), renderAstGrepTest(r)],
          ] as const) {
            mkdirSync(dirname(join(dir, rel)), { recursive: true });
            writeFileSync(join(dir, rel), text);
          }
        }
        const res = run(cmd[0]!, [...cmd.slice(1), "test", "--skip-snapshot-tests", "-c", join(dir, "sgconfig.yml")], { cwd: dir });
        const out = res.stdout + res.stderr;
        const ran = /Running (\d+) tests/.exec(out);
        const files = readdirSync(join(dir, "rules"), { recursive: true }).filter((f) => String(f).endsWith(".yml")).length;
        if (!ran) failures.push(`ast-grep test did not run:\n${out.trim()}`);
        else if (Number(ran[1]) !== files) failures.push(`ast-grep loaded ${ran[1]} rules, expected ${files}`);
        if (res.status !== 0) {
          const failed = out
            .split("\n")
            .filter((l) => /^(FAIL|\[|Error|error)/.test(l.trim()) || /incorrect|Noisy|Missing/i.test(l))
            .slice(0, 40);
          failures.push(`ast-grep test failed:\n${(failed.length ? failed : out.trim().split("\n").slice(-30)).join("\n")}`);
        }
      } finally {
        rmSync(dir, { recursive: true, force: true });
      }
    }
  }

  // guard examples, each rule in isolation, on every available shell
  const guardRules = rules.filter((r) => r.enforce.some((t) => t.target === "guard"));
  counts.guard = guardRules.length;
  if (guardRules.length) {
    const shells = availableShells();
    for (const s of ["sh", "dash", "bash --posix", "busybox sh"])
      if (!shells.some((x) => x.name === s)) notes.push(`guard: ${s} not available, skipped`);
    const dir = mkdtempSync(join(tmpdir(), "forge-guard-"));
    try {
      writeFileSync(join(dir, "guard.sh"), template("guard.sh"));
      for (const r of guardRules) {
        const targets = r.enforce.filter((t): t is GuardTarget => t.target === "guard");
        writeFileSync(join(dir, "guard.rules"), renderGuardRules(targets.map((target) => ({ rule: r, target })), "/"));
        const decision = targets.some((t) => t.decision === "deny") ? "deny" : "ask";
        for (const sh of shells) {
          for (const ex of r.examples?.match ?? []) {
            const got = guardDecision(sh.argv, dir, ex);
            if (got !== decision) failures.push(`${r.id} [${sh.name}]: expected ${decision} for ${JSON.stringify(ex)}, got ${got || "nothing"}`);
          }
          for (const ex of r.examples?.no_match ?? []) {
            const got = guardDecision(sh.argv, dir, ex);
            if (got !== "") failures.push(`${r.id} [${sh.name}]: expected nothing for ${JSON.stringify(ex)}, got ${got}`);
          }
        }
      }
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  }

  // permission examples
  const permRules = rules.filter((r) => r.enforce.some((t) => t.target === "permission"));
  counts.permission = permRules.length;
  for (const r of permRules) {
    const specs = r.enforce.filter((t): t is PermissionTarget => t.target === "permission").map((t) => t.rule);
    for (const ex of r.examples?.match ?? [])
      if (!specs.some((s) => permissionMatches(s, ex))) failures.push(`${r.id}: no permission rule matches ${ex}`);
    for (const ex of r.examples?.no_match ?? []) {
      const hit = specs.find((s) => permissionMatches(s, ex));
      if (hit) failures.push(`${r.id}: ${hit} matches no_match example ${ex}`);
    }
  }

  if (!existsSync(packsDir)) failures.push(`${packsDir} does not exist`);
  return { failures, notes, counts };
}
