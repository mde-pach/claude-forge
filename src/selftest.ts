import { existsSync, mkdirSync, mkdtempSync, readdirSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { delimiter, dirname, join } from "node:path";
import { examplePath } from "./audit.js";
import { codeExamples, globMatch, loadPacks, loadLocalRules } from "./packs.js";
import {
  astGrepRulePath,
  astGrepTestPath,
  checkTools,
  PROJECT_TOOLS,
  PYTHON_TOOLS,
  renderAstGrepRule,
  renderAstGrepTest,
  renderCheckScript,
  renderGuardRules,
  renderSgConfig,
  template,
} from "./render.js";
import { AST_GREP_VERSION, ownedHash, run } from "./util.js";
import type { AstGrepTarget, CheckTarget, Example, ExampleFixture, GuardTarget, PermissionTarget, Rule } from "./types.js";

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

// ---------------------------------------------------------------- check examples

/**
 * Check targets that need a whole project the examples cannot stand in for:
 * test runs, migrations, `astro check` and the lock-file checks. Their
 * examples are optional.
 */
export function checkExamplesOptional(t: CheckTarget): boolean {
  return t.stage === "test" || t.stage === "migration" || checkTools(t).some((x) => x === "astro" || x === "uv" || x === "bun");
}

/** How check examples are run: format checks fix files at Stop, so they are run as CI runs them. */
export function checkExampleMode(t: CheckTarget): "--changed" | "--all" {
  return t.stage === "format" ? "--all" : "--changed";
}

const GIT_ENV = { GIT_AUTHOR_NAME: "forge", GIT_AUTHOR_EMAIL: "forge@localhost", GIT_COMMITTER_NAME: "forge", GIT_COMMITTER_EMAIL: "forge@localhost", GIT_CONFIG_NOSYSTEM: "1" };

function writeFiles(dir: string, files: Record<string, string>): void {
  for (const [rel, text] of Object.entries(files)) {
    mkdirSync(dirname(join(dir, rel)), { recursive: true });
    writeFileSync(join(dir, rel), text);
  }
}

/** The example as a fixture: a string is written to a file inside the rule's scope. */
export function exampleFixture(rule: Rule, ex: Example): ExampleFixture {
  return typeof ex === "string" ? { files: { [examplePath(rule, "example")]: ex } } : ex;
}

export interface CheckExampleResult {
  /** "pass" (behaved as declared), "fail" (did not), "skip" (a tool is missing). */
  outcome: "pass" | "fail" | "skip";
  detail: string;
}

/**
 * Runs one example of a check target: a temporary git repository holding the
 * fixture's `head` files (committed) with only this check in `.forge/check.sh`,
 * then the fixture's `files` written and staged; check.sh runs in
 * checkExampleMode(). A `match` example must fail the check, a `no_match`
 * example must pass it. Project tools come from `toolBins`, system tools from PATH.
 */
export function runCheckExample(rule: Rule, t: CheckTarget, ex: Example, kind: "match" | "no_match", toolBins: string[]): CheckExampleResult {
  const dir = mkdtempSync(join(tmpdir(), "forge-check-example-"));
  try {
    const fixture = exampleFixture(rule, ex);
    const script = renderCheckScript([{ rule, target: t }], { forbid: "", noIncrease: "", noDecrease: "" });
    const owned = { ".forge/check.sh": script, ".forge/.gitignore": "state/\n" };
    const manifest = Object.entries(owned).map(([p, text]) => `${ownedHash(p, text)}  ${p}`).join("\n") + "\n";
    writeFiles(dir, { ".gitignore": "node_modules/\n.venv/\n", ...owned, ".forge/manifest.sha256": manifest, ...(fixture.head ?? {}) });
    for (const tool of checkTools(t)) {
      if ((PROJECT_TOOLS as readonly string[]).includes(tool) || PYTHON_TOOLS.has(tool)) {
        const bin = toolBins.map((b) => join(b, tool)).find((p) => existsSync(p));
        if (!bin) return { outcome: "skip", detail: `${tool} not available` };
        const to = join(dir, PYTHON_TOOLS.has(tool) ? ".venv/bin" : "node_modules/.bin", tool);
        mkdirSync(dirname(to), { recursive: true });
        symlinkSync(bin, to);
      } else if (run("sh", ["-c", `command -v ${tool}`]).status !== 0) return { outcome: "skip", detail: `${tool} not available` };
    }
    const git = (...args: string[]) => run("git", args, { cwd: dir, env: { ...process.env, ...GIT_ENV } });
    git("init", "-q", "-b", "main");
    git("add", "-A");
    git("commit", "-qm", "head", "--allow-empty");
    writeFiles(dir, fixture.files);
    git("add", "-A");
    const r = run("sh", [".forge/check.sh", checkExampleMode(t)], { cwd: dir });
    const out = (r.stdout + r.stderr).trim();
    const skipped = new RegExp(`^forge:${rule.id.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")} skipped: (.*)$`, "m").exec(out);
    if (skipped) return { outcome: "skip", detail: skipped[1]! };
    const failed = r.status !== 0 && out.includes(`forge:${rule.id} failed`);
    const brief = out.split("\n").slice(0, 6).join(" | ") || "(no output)";
    if (kind === "match") return failed ? { outcome: "pass", detail: "" } : { outcome: "fail", detail: `did not fail the check: ${brief}` };
    return r.status === 0 ? { outcome: "pass", detail: "" } : { outcome: "fail", detail: `failed the check: ${brief}` };
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

/** Bin directories that hold the project tools for check examples: the project's, then PATH. */
export function defaultToolBins(repo?: string): string[] {
  return [...(repo ? [join(repo, "node_modules", ".bin"), join(repo, ".venv", "bin")] : []), ...(process.env.PATH ?? "").split(delimiter).filter(Boolean)];
}

export function selfTest(packsDir: string, repo?: string, opts: { toolBins?: string[] } = {}): TestReport {
  const failures: string[] = [];
  const notes: string[] = [];
  const counts: Record<string, number> = { packs: 0, rules: 0, astgrep: 0, guard: 0, permission: 0, check: 0, context: 0 };

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
    const needsExamples = r.enforce.some(
      (t) => ["ast-grep", "guard", "permission", "tool"].includes(t.target) || (t.target === "check" && !checkExamplesOptional(t)),
    );
    if (needsExamples && !(r.examples?.match?.length && r.examples?.no_match?.length))
      failures.push(`${r.id}: needs match and no_match examples`);
    const fixtures = [...(r.examples?.match ?? []), ...(r.examples?.no_match ?? [])].some((e) => typeof e !== "string");
    if (fixtures && r.enforce.some((t) => t.target !== "check")) failures.push(`${r.id}: fixture examples (files) are for check rules only`);
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
          for (const ex of codeExamples(r, "match")) {
            const got = guardDecision(sh.argv, dir, ex);
            if (got !== decision) failures.push(`${r.id} [${sh.name}]: expected ${decision} for ${JSON.stringify(ex)}, got ${got || "nothing"}`);
          }
          for (const ex of codeExamples(r, "no_match")) {
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
    for (const ex of codeExamples(r, "match"))
      if (!specs.some((s) => permissionMatches(s, ex))) failures.push(`${r.id}: no permission rule matches ${ex}`);
    for (const ex of codeExamples(r, "no_match")) {
      const hit = specs.find((s) => permissionMatches(s, ex));
      if (hit) failures.push(`${r.id}: ${hit} matches no_match example ${ex}`);
    }
  }

  // check examples, each in its own temporary repository
  const checkRules = rules.filter((r) => r.enforce.some((t) => t.target === "check") && (r.examples?.match?.length || r.examples?.no_match?.length));
  counts.check = checkRules.length;
  const toolBins = opts.toolBins ?? defaultToolBins(repo);
  for (const r of checkRules)
    for (const t of r.enforce.filter((x): x is CheckTarget => x.target === "check"))
      for (const kind of ["match", "no_match"] as const)
        (r.examples?.[kind] ?? []).forEach((ex, i) => {
          const res = runCheckExample(r, t, ex, kind, toolBins);
          if (res.outcome === "fail") failures.push(`${r.id}: ${kind} example ${i + 1} ${res.detail}`);
          else if (res.outcome === "skip") notes.push(`${r.id}: ${kind} example ${i + 1} skipped: ${res.detail}`);
        });

  if (!existsSync(packsDir)) failures.push(`${packsDir} does not exist`);
  return { failures, notes, counts };
}
