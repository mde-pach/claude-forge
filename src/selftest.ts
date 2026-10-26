import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import { delimiter, dirname, join } from "node:path";
import { examplePath } from "./audit.js";
import { codeExamples, globMatch, guardExamples, loadPacks, loadLocalRules } from "./packs.js";
import {
  astGrepRulePath,
  astGrepTestPath,
  checkTools,
  PROJECT_TOOLS,
  PYTHON_TOOLS,
  renderAstGrepRule,
  renderAstGrepTest,
  renderCheckScript,
  renderChecks,
  helperFiles,
  renderGuardRules,
  renderLib,
  renderSgConfig,
  template,
  typesLanguage,
  type TypesLanguage,
} from "./render.js";
import { AST_GREP_VERSION, ENGINE_PYTHON, HADOLINT_SHA256, HADOLINT_VERSION, type Job, ownedHash, run, runParallel, sha256 } from "./util.js";
import type { AstGrepTarget, CheckTarget, DepSpec, Example, ExampleFixture, GrepTarget, GuardTarget, PermissionTarget, Rule, WhereCondition } from "./types.js";

export interface TestReport {
  failures: string[];
  notes: string[];
  counts: Record<string, number>;
}

// ---------------------------------------------------------------- ast-grep

/** argv prefix that runs the pinned ast-grep, or undefined. */
/** The counts line of a test run, after `what` (packs and rules, or local rules). */
export function testSummary(c: Record<string, number>, what: string): string {
  return `${what}; ast-grep ${c.astgrep}, guard ${c.guard}, permission ${c.permission}, grep ${c.grep}, check ${c.check}, context ${c.context}`;
}

export function astGrepCommand(): string[] | undefined {
  if (run("bunx", ["--version"]).status === 0) return ["bunx", "-p", `@ast-grep/cli@${AST_GREP_VERSION}`, "ast-grep"];
  if (run("uvx", ["--version"]).status === 0) return ["uvx", "-q", "--from", `ast-grep-cli==${AST_GREP_VERSION}`, "ast-grep"];
  return undefined;
}

// ---------------------------------------------------------------- guard

export function hookInput(command: string, cwd = "/"): string {
  return JSON.stringify({
    session_id: "forge-test",
    cwd,
    hook_event_name: "PreToolUse",
    tool_name: "Bash",
    tool_input: { command, description: "test" },
  });
}

/** Runs guard.sh from <dir> under <shell> (argv) on <command> (from <cwd>); returns "deny" | "ask" | "" (or "error: …"). */
export function guardDecision(shell: string[], dir: string, command: string, cwd = "/"): string {
  return guardOutcome(run(shell[0]!, [...shell.slice(1), join(dir, "guard.sh")], { input: hookInput(command, cwd) }));
}

/** The guard.sh run of guardDecision. */
function guardJob(shell: string[], dir: string, command: string, cwd = "/"): Job {
  return { cmd: shell[0]!, args: [...shell.slice(1), join(dir, "guard.sh")], input: hookInput(command, cwd) };
}

/** guardDecision's reading of a guard.sh run. */
function guardOutcome(r: { status: number; stdout: string; stderr: string }): string {
  if (r.status !== 0) return `error: exit ${r.status} ${r.stderr.trim()}`;
  if (!r.stdout.trim()) return "";
  try {
    const out = JSON.parse(r.stdout) as { hookSpecificOutput?: { permissionDecision?: string } };
    return out.hookSpecificOutput?.permissionDecision ?? `error: ${r.stdout.trim()}`;
  } catch {
    return `error: not JSON: ${r.stdout.trim()}`;
  }
}

/**
 * A directory to run a guard example from: a repository root (it holds .git)
 * with the example's files, executable (node_modules/.bin/<tool>, .venv/bin/<tool>).
 */
export function guardExampleDir(files: Record<string, string> = {}): string {
  const dir = mkdtempSync(join(tmpdir(), "forge-guard-ex-"));
  mkdirSync(join(dir, ".git"));
  for (const [rel, text] of Object.entries(files)) {
    mkdirSync(dirname(join(dir, rel)), { recursive: true });
    writeFileSync(join(dir, rel), text, { mode: 0o755 });
  }
  return dir;
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

// ---------------------------------------------------------------- hadolint

/** Whether the pinned hadolint-bin wheel ships the upstream release binary for this platform. */
export function hadolintIntegrity(): { status: "ok" | "mismatch" | "skipped"; detail: string } {
  if (run("uvx", ["--version"]).status !== 0) return { status: "skipped", detail: "uvx is not available" };
  const which = run("uvx", ["-q", "--from", `hadolint-bin==${HADOLINT_VERSION}`, "python", "-c", 'import shutil; print(shutil.which("hadolint") or "")']);
  const bin = which.stdout.trim();
  if (which.status !== 0 || !bin) return { status: "skipped", detail: `hadolint-bin ${HADOLINT_VERSION} could not be installed` };
  const platform = `${run("uname", ["-s"]).stdout.trim()}-${run("uname", ["-m"]).stdout.trim()}`;
  const want = HADOLINT_SHA256[platform];
  if (!want) return { status: "skipped", detail: `no upstream checksum for ${platform}` };
  const have = sha256(readFileSync(bin));
  return have === want
    ? { status: "ok", detail: `${platform} ${have}` }
    : { status: "mismatch", detail: `hadolint-bin ${HADOLINT_VERSION} ships ${have} for ${platform}, the upstream release is ${want}` };
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

/** How check examples are run: as Stop runs them, except format checks, which fix files at Stop and are run as CI runs them. */
export function checkExampleMode(t: CheckTarget): "--turn" | "--all" {
  return t.stage === "format" ? "--all" : "--turn";
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
  if (typeof ex === "string") return { files: { [examplePath(rule, "example")]: ex } };
  if ("command" in ex) throw new Error(`${rule.id}: a command example is for guard rules only`);
  return ex;
}

export interface CheckExampleResult {
  /** "pass" (behaved as declared), "fail" (did not), "skip" (a tool is missing). */
  outcome: "pass" | "fail" | "skip";
  detail: string;
}

/**
 * Runs one example of a check target: a temporary git repository holding the
 * fixture's `head` files (committed) with only this check in `.forge/check.sh`,
 * then the fixture's `files` written and staged, as a turn that started at the
 * head; check.sh runs in checkExampleMode(). A `match` example must fail the check, a `no_match`
 * example must pass it. Project tools come from `toolBins`, system tools from PATH.
 */
export function runCheckExample(rule: Rule, t: CheckTarget, ex: Example, kind: "match" | "no_match", toolBins: string[]): CheckExampleResult {
  return runCheckExamples([{ rule, target: t, example: ex, kind }], toolBins)[0]!;
}

export interface CheckExampleRun {
  rule: Rule;
  target: CheckTarget;
  example: Example;
  kind: "match" | "no_match";
}

const SNAP = ".forge/state/example.turn";
/**
 * One example's run, after its repository holds the head files: commits them (without
 * git's automatic maintenance, which a commit starts in the background: a throwaway
 * repository needs none), records the turn's start (the snapshot turn-start.sh would
 * write: that head, a clean tree), writes the fixture's files (staged in $1) and stages
 * them, then runs check.sh (its mode in $2…).
 */
const EXAMPLE_RUN = `g() ( export ${Object.entries(GIT_ENV).map(([k, v]) => `${k}=${v}`).join(" ")}; exec git -c maintenance.auto=false "$@" )
g init -q -b main
g add -A
g commit -qm head --allow-empty
mkdir -p ${SNAP}
g rev-parse HEAD > ${SNAP}/head
printf '\\n' > ${SNAP}/tree
: > ${SNAP}/dirty
stage=$1; shift
cp -R "$stage/." .
g add -A
exec sh .forge/check.sh "$@"
`;

/** Runs check examples (runCheckExample), several at a time: each in its own repository. */
export function runCheckExamples(runs: CheckExampleRun[], toolBins: string[]): CheckExampleResult[] {
  const results: (CheckExampleResult | undefined)[] = [];
  const jobs: Job[] = [];
  const pending: number[] = [];
  const dirs: string[] = [];
  try {
    runs.forEach(({ rule, target: t, example: ex }, i) => {
      const dir = mkdtempSync(join(tmpdir(), "forge-check-example-"));
      dirs.push(dir);
      const repo = join(dir, "repo");
      const stage = join(dir, "files");
      const fixture = exampleFixture(rule, ex);
      const script = renderCheckScript([{ rule, target: t }], { forbid: "", noIncrease: "", noDecrease: "" });
      const owned: Record<string, string> = {
        ".forge/check.sh": script,
        ".forge/.gitignore": "state/\n",
        ".forge/ast-grep/sgconfig.yml": renderSgConfig(),
        ...Object.fromEntries(helperFiles([{ rule, target: t }])),
      };
      const manifest = Object.entries(owned).map(([p, text]) => `${ownedHash(p, text)}  ${p}`).join("\n") + "\n";
      mkdirSync(repo);
      mkdirSync(stage);
      writeFiles(repo, { ".gitignore": "node_modules/\n.venv/\n", ...owned, ".forge/manifest.sha256": manifest, ...(fixture.head ?? {}) });
      writeFiles(stage, fixture.files);
      for (const tool of checkTools(t)) {
        if ((PROJECT_TOOLS as readonly string[]).includes(tool) || PYTHON_TOOLS.has(tool)) {
          const bin = toolBins.map((b) => join(b, tool)).find((p) => existsSync(p));
          if (!bin) return void (results[i] = { outcome: "skip", detail: `${tool} not available` });
          const to = join(repo, PYTHON_TOOLS.has(tool) ? ".venv/bin" : "node_modules/.bin", tool);
          mkdirSync(dirname(to), { recursive: true });
          symlinkSync(bin, to);
        } else if (run("sh", ["-c", `command -v ${tool === "ty" ? "uvx" : tool}`]).status !== 0) return void (results[i] = { outcome: "skip", detail: `${tool === "ty" ? "uvx (for ty)" : tool} not available` });
      }
      const mode = checkExampleMode(t);
      // its own TMPDIR: bunx installs ast-grep there at every run, and two at once in one place fail
      const tmp = join(dir, "tmp");
      mkdirSync(tmp);
      jobs.push({ cmd: "sh", args: ["-c", EXAMPLE_RUN, "sh", stage, ...(mode === "--turn" ? [mode, SNAP] : [mode])], cwd: repo, env: { TMPDIR: `${tmp}/` } });
      pending.push(i);
    });
    runParallel(jobs).forEach((r, j) => {
      const i = pending[j]!;
      const { rule, kind } = runs[i]!;
      const out = (r.stdout + r.stderr).trim();
      const skipped = new RegExp(`^forge:${rule.id.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")} skipped: (.*)$`, "m").exec(out);
      if (skipped) return void (results[i] = { outcome: "skip", detail: skipped[1]! });
      const failed = r.status !== 0 && out.includes(`forge:${rule.id} failed`);
      const brief = out.split("\n").slice(0, 6).join(" | ") || "(no output)";
      if (kind === "match") results[i] = failed ? { outcome: "pass", detail: "" } : { outcome: "fail", detail: `did not fail the check: ${brief}` };
      else results[i] = r.status === 0 ? { outcome: "pass", detail: "" } : { outcome: "fail", detail: `failed the check: ${brief}` };
    });
    return results as CheckExampleResult[];
  } finally {
    for (const d of dirs) rmSync(d, { recursive: true, force: true });
  }
}

/** Bin directories that hold the project tools for check examples: the project's, then PATH. */
export function defaultToolBins(repo?: string): string[] {
  return [...(repo ? [join(repo, "node_modules", ".bin"), join(repo, ".venv", "bin")] : []), ...(process.env.PATH ?? "").split(delimiter).filter(Boolean)];
}

/**
 * The development self-test (`bun run test:packs`, the spec suite): every pack and rule
 * of packsDir (and, with repo, its local rules replacing or adding to them) validated
 * and their examples run.
 */
export function selfTest(packsDir: string, repo?: string, opts: { toolBins?: string[] } = {}): TestReport {
  const failures: string[] = [];
  const notes: string[] = [];
  const counts: Record<string, number> = { packs: 0, rules: 0, astgrep: 0, guard: 0, permission: 0, grep: 0, check: 0, context: 0 };

  const rules: Rule[] = [];
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
  testRules(rules, repo, opts, { failures, notes, counts });
  if (!existsSync(packsDir)) failures.push(`${packsDir} does not exist`);
  return { failures, notes, counts };
}

/**
 * `forge test` in a project: the rules of .forge/rules only (a local rule replacing a
 * pack rule is tested as written), validated and their examples run. The bundled
 * packs are proved by Forge's own test suite.
 */
export function localSelfTest(repo: string, opts: { toolBins?: string[] } = {}): TestReport {
  const failures: string[] = [];
  const notes: string[] = [];
  const counts: Record<string, number> = { packs: 0, rules: 0, astgrep: 0, guard: 0, permission: 0, grep: 0, check: 0, context: 0 };
  let rules: Rule[];
  try {
    rules = loadLocalRules(repo);
  } catch (e) {
    failures.push((e as Error).message);
    return { failures, notes, counts };
  }
  testRules(rules, repo, opts, { failures, notes, counts });
  return { failures, notes, counts };
}

/** Structural checks and every example of the given rules. */
function testRules(rules: Rule[], repo: string | undefined, opts: { toolBins?: string[] }, report: TestReport): void {
  const { failures, notes, counts } = report;
  counts.rules = rules.length;

  // structural checks the schema cannot express
  for (const r of rules) {
    const ast = r.enforce.filter((t) => t.target === "ast-grep");
    if (ast.length > 1) failures.push(`${r.id}: at most one ast-grep target`);
    if (ast.length && r.scope.language === "any") failures.push(`${r.id}: ast-grep targets need a concrete scope.language`);
    const needsExamples = r.enforce.some(
      (t) => ["ast-grep", "guard", "permission", "tool", "grep"].includes(t.target) || (t.target === "check" && !checkExamplesOptional(t)),
    );
    if (needsExamples && !(r.examples?.match?.length && r.examples?.no_match?.length))
      failures.push(`${r.id}: needs match and no_match examples`);
    const all = [...(r.examples?.match ?? []), ...(r.examples?.no_match ?? [])];
    if (all.some((e) => typeof e !== "string" && !("command" in e)) && r.enforce.some((t) => t.target !== "check"))
      failures.push(`${r.id}: fixture examples (files) are for check rules only`);
    if (all.some((e) => typeof e !== "string" && "command" in e) && r.enforce.some((t) => t.target !== "guard"))
      failures.push(`${r.id}: command examples are for guard rules only`);
    // engine capabilities (§15.1): metavariables of the rule, a parsed language, examples with their packages
    const typed = ast.find((t): t is AstGrepTarget => t.target === "ast-grep" && !!(t as AstGrepTarget).where);
    if (typed) {
      const text = JSON.stringify([typed.rule, typed.constraints ?? {}, typed.utils ?? {}]);
      for (const v of Object.keys(typed.where!)) if (!text.includes(`$${v}`)) failures.push(`${r.id}: where: $${v} is not a metavariable of the rule`);
      if (!["python", "typescript", "tsx"].includes(r.scope.language)) failures.push(`${r.id}: where needs scope.language python, typescript or tsx`);
      const eco = typesLanguage(r, typed) === "python" ? "pypi" : "npm";
      if (!r.examples?.deps?.length) failures.push(`${r.id}: where needs examples.deps (the packages its examples are type-checked against)`);
      else if (r.examples.deps.some((d) => d.ecosystem !== eco)) failures.push(`${r.id}: examples.deps must be ${eco} packages`);
    } else if (r.examples?.deps) failures.push(`${r.id}: examples.deps is for rules with a where`);
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

  // examples of rules with type constraints, through the types engine (§15.1)
  const typedRules = astRules.filter((r) => r.enforce.some((t) => t.target === "ast-grep" && t.where) && r.examples?.deps?.length);
  counts.types = typedRules.length;
  if (typedRules.length) typesExamples(typedRules, { failures, notes, counts });

  // guard examples, each rule in isolation, on every available shell
  const guardRules = rules.filter((r) => r.enforce.some((t) => t.target === "guard"));
  counts.guard = guardRules.length;
  if (guardRules.length) {
    const shells = availableShells();
    for (const s of ["sh", "dash", "bash --posix", "busybox sh"])
      if (!shells.some((x) => x.name === s)) notes.push(`guard: ${s} not available, skipped`);
    // each rule in its own directory, every example on every shell, several at a time
    const dirs: string[] = [];
    const runs: { job: Job; report: (got: string) => void }[] = [];
    try {
      for (const r of guardRules) {
        const dir = mkdtempSync(join(tmpdir(), "forge-guard-"));
        dirs.push(dir);
        writeFileSync(join(dir, "guard.sh"), template("guard.sh"));
        const targets = r.enforce.filter((t): t is GuardTarget => t.target === "guard");
        writeFileSync(join(dir, "guard.rules"), renderGuardRules(targets.map((target) => ({ rule: r, target })), "/"));
        const decision = targets.some((t) => t.decision === "deny") ? "deny" : "ask";
        for (const kind of ["match", "no_match"] as const)
          for (const ex of guardExamples(r, kind)) {
            const cwd = guardExampleDir(ex.files);
            dirs.push(cwd);
            for (const sh of shells) {
              const want = kind === "match" ? decision : "";
              runs.push({
                job: guardJob(sh.argv, dir, ex.command, cwd),
                report: (got) => {
                  if (got !== want)
                    failures.push(`${r.id} [${sh.name}]: expected ${want || "nothing"} for ${JSON.stringify(ex.command)}${ex.files ? ` with ${Object.keys(ex.files).join(", ")}` : ""}, got ${got || "nothing"}`);
                },
              });
            }
          }
      }
      runParallel(runs.map((x) => x.job)).forEach((res, i) => runs[i]!.report(guardOutcome(res)));
    } finally {
      for (const d of dirs) rmSync(d, { recursive: true, force: true });
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

  // grep examples: the pattern (grep -E) matches a line of every match example and none of a no_match example
  const grepRules = rules.filter((r) => r.enforce.some((t) => t.target === "grep"));
  counts.grep = grepRules.length;
  if (grepRules.length) {
    const dir = mkdtempSync(join(tmpdir(), "forge-grep-"));
    try {
      for (const r of grepRules)
        for (const t of r.enforce.filter((x): x is GrepTarget => x.target === "grep"))
          for (const kind of ["match", "no_match"] as const)
            codeExamples(r, kind).forEach((ex, i) => {
              writeFileSync(join(dir, "example"), ex);
              const n = Number(run("sh", ["-c", 'grep -c "$3" -- "$1" "$2"', "sh", t.pattern, join(dir, "example"), t.ignore_case ? "-iE" : "-E"]).stdout.trim() || "0");
              if (kind === "match" && n === 0) failures.push(`${r.id}: grep pattern does not match match example ${i + 1}: ${JSON.stringify(ex)}`);
              if (kind === "no_match" && n > 0) failures.push(`${r.id}: grep pattern matches no_match example ${i + 1}: ${JSON.stringify(ex)}`);
            });
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  }

  // check examples, each in its own temporary repository, several at a time
  const checkRules = rules.filter((r) => r.enforce.some((t) => t.target === "check") && (r.examples?.match?.length || r.examples?.no_match?.length));
  counts.check = checkRules.length;
  const toolBins = opts.toolBins ?? defaultToolBins(repo);
  const runs: (CheckExampleRun & { n: number })[] = [];
  for (const r of checkRules)
    for (const t of r.enforce.filter((x): x is CheckTarget => x.target === "check"))
      for (const kind of ["match", "no_match"] as const) (r.examples?.[kind] ?? []).forEach((ex, i) => runs.push({ rule: r, target: t, example: ex, kind, n: i + 1 }));
  runCheckExamples(runs, toolBins).forEach((res, j) => {
    const { rule: r, kind, n } = runs[j]!;
    if (res.outcome === "fail") failures.push(`${r.id}: ${kind} example ${n} ${res.detail}`);
    else if (res.outcome === "skip") notes.push(`${r.id}: ${kind} example ${n} skipped: ${res.detail}`);
  });

  // hadolint: the hadolint-bin wheel's binary is the upstream release binary (§13.3)
  if (rules.some((r) => r.enforce.some((t) => t.target === "check" && /\bforge_hadolint\b/.test(t.run)))) {
    const h = hadolintIntegrity();
    if (h.status === "mismatch") failures.push(`hadolint: ${h.detail}`);
    else if (h.status === "skipped") notes.push(`hadolint: ${h.detail}`);
  }
}


// ---------------------------------------------------------------- types (§15.1)

/** The TypeScript that type-checks a rule's examples when its examples.deps name none. */
const EXAMPLES_TYPESCRIPT = "7.0.2";

/** Where example projects (their packages installed) are kept between runs. */
function examplesCache(): string {
  return process.env.FORGE_EXAMPLES_CACHE ?? join(process.env.XDG_CACHE_HOME ?? join(homedir(), ".cache"), "forge", "examples");
}

/**
 * A project holding the given packages, for the types engine: a uv virtual environment
 * on the engine's Python (pypi), or a package.json installed with bun (npm). Created
 * once per set of packages. Returns its directory, or an error.
 */
export function examplesProject(lang: TypesLanguage, deps: DepSpec[]): { dir?: string; error?: string } {
  const key = sha256(JSON.stringify([lang, [...deps].sort((a, b) => (a.name < b.name ? -1 : 1)), lang === "typescript" ? EXAMPLES_TYPESCRIPT : ""])).slice(0, 16);
  const dir = join(examplesCache(), `${lang}-${key}`);
  const done = join(dir, ".forge-ready");
  if (existsSync(done)) return { dir };
  rmSync(dir, { recursive: true, force: true });
  mkdirSync(dir, { recursive: true });
  if (lang === "python") {
    writeFileSync(join(dir, "pyproject.toml"), `[project]\nname = "forge-examples"\nversion = "0"\nrequires-python = ">=${ENGINE_PYTHON}"\n`);
    let r = run("uv", ["venv", "-q", "--python", ENGINE_PYTHON, join(dir, ".venv")]);
    if (r.status === 0) r = run("uv", ["pip", "install", "-q", "--python", join(dir, ".venv", "bin", "python"), ...deps.map((d) => `${d.name}==${d.version}`)]);
    if (r.status !== 0) return { error: `uv: ${(r.stderr || r.stdout).trim().split("\n").slice(-3).join(" ")}` };
  } else {
    // the types engine is the project's TypeScript (§15.2): the examples get one unless deps name it
    const dependencies = { typescript: EXAMPLES_TYPESCRIPT, ...Object.fromEntries(deps.map((d) => [d.name, d.version])) };
    writeFileSync(join(dir, "package.json"), JSON.stringify({ name: "forge-examples", private: true, type: "module", dependencies }, null, 2));
    writeFileSync(join(dir, "tsconfig.json"), JSON.stringify({ compilerOptions: { target: "ES2022", module: "ESNext", moduleResolution: "bundler", strict: true, noEmit: true, skipLibCheck: true } }, null, 2));
    const r = run("bun", ["install", "--silent"], { cwd: dir });
    if (r.status !== 0) return { error: `bun install: ${(r.stderr || r.stdout).trim().split("\n").slice(-3).join(" ")}` };
  }
  writeFileSync(done, "");
  return { dir };
}

/**
 * Runs the forge_types_filter of the generated scripts on ast-grep matches (a file of
 * --json=stream lines, paths relative to project), with a state directory of its own;
 * the daemon it starts is stopped at the end. Returns the matches kept and the output.
 */
export function runTypesFilter(project: string, lang: TypesLanguage, matches: string, state: string): { kept: string[]; output: string; status: number } {
  const script = [
    renderLib(),
    "IFS='\n'",
    renderChecks([], [], [lang]),
    `FORGE_STATE=${JSON.stringify(state)}`,
    'forge_types_filter "$1"; rc=$?',
    "forge_types_down",
    'exit "$rc"',
  ].join("\n");
  const file = join(state, "filter.sh");
  mkdirSync(state, { recursive: true, mode: 0o700 });
  // the engine's adapter, where the generated scripts start it from
  for (const [rel, text] of helperFiles([], [lang])) {
    mkdirSync(dirname(join(project, rel)), { recursive: true });
    writeFileSync(join(project, rel), text);
  }
  writeFileSync(file, script);
  const r = run("sh", [file, matches], { cwd: project });
  return { kept: readFileSync(matches, "utf8").split("\n").filter(Boolean), output: r.stdout + r.stderr, status: r.status };
}

function typesExamples(rules: Rule[], report: TestReport): void {
  const { failures, notes } = report;
  const cmd = astGrepCommand();
  if (!cmd) return; // reported by the ast-grep tests
  const groups = new Map<string, { lang: TypesLanguage; deps: DepSpec[]; rules: Rule[] }>();
  for (const r of rules) {
    const t = r.enforce.find((x): x is AstGrepTarget => x.target === "ast-grep" && !!x.where)!;
    const lang = typesLanguage(r, t)!;
    const key = JSON.stringify([lang, r.examples!.deps]);
    if (!groups.has(key)) groups.set(key, { lang, deps: r.examples!.deps!, rules: [] });
    groups.get(key)!.rules.push(r);
  }
  for (const { lang, deps, rules: group } of groups.values()) {
    const project = examplesProject(lang, deps);
    if (!project.dir) {
      failures.push(`types examples (${deps.map((d) => `${d.name}@${d.version}`).join(", ")}): ${project.error}`);
      continue;
    }
    const sg = mkdtempSync(join(tmpdir(), "forge-types-sg-"));
    const state = mkdtempSync(join(tmpdir(), "forge-types-state-"));
    const ex = mkdtempSync(join(project.dir, "ex-"));
    const rel = ex.slice(project.dir.length + 1);
    try {
      writeFileSync(join(sg, "sgconfig.yml"), renderSgConfig());
      const expected = new Map<string, { rule: Rule; kind: "match" | "no_match"; i: number }>();
      for (const r of group) {
        const t = r.enforce.find((x): x is AstGrepTarget => x.target === "ast-grep")!;
        mkdirSync(dirname(join(sg, astGrepRulePath(r.id))), { recursive: true });
        writeFileSync(join(sg, astGrepRulePath(r.id)), renderAstGrepRule(r, t));
        for (const kind of ["match", "no_match"] as const)
          codeExamples(r, kind).forEach((code, i) => {
            const name = `${rel}/${r.id.replaceAll("/", "_")}_${kind}_${i + 1}.${lang === "python" ? "py" : "ts"}`;
            writeFileSync(join(project.dir!, name), code);
            expected.set(name, { rule: r, kind, i });
          });
      }
      const scan = run(cmd[0]!, [...cmd.slice(1), "scan", "-c", join(sg, "sgconfig.yml"), "--json=stream", "--include-metadata", ...expected.keys()], { cwd: project.dir });
      const matches = join(state, "matches.json");
      writeFileSync(matches, scan.stdout);
      const res = runTypesFilter(project.dir, lang, matches, join(state, "s"));
      if (res.status !== 0) {
        failures.push(`types examples (${lang}): the types engine failed: ${res.output.trim().split("\n").slice(0, 5).join(" | ")}`);
        continue;
      }
      const hit = new Map<string, Set<string>>();
      for (const l of res.kept) {
        const m = JSON.parse(l) as { file: string; ruleId: string };
        if (!hit.has(m.file)) hit.set(m.file, new Set());
        hit.get(m.file)!.add(m.ruleId);
      }
      for (const [file, { rule, kind, i }] of expected) {
        const got = hit.get(file)?.has(rule.id) ?? false;
        if (kind === "match" && !got) failures.push(`${rule.id}: match example ${i + 1} is not matched with types (${describeWhere(rule)})`);
        if (kind === "no_match" && got) failures.push(`${rule.id}: no_match example ${i + 1} is matched with types (${describeWhere(rule)})`);
      }
      notes.push(`types (${lang}): ${expected.size} examples of ${group.length} rule(s) checked with ${deps.map((d) => `${d.name}@${d.version}`).join(", ")}`);
    } finally {
      rmSync(sg, { recursive: true, force: true });
      rmSync(state, { recursive: true, force: true });
      rmSync(ex, { recursive: true, force: true });
    }
  }
}

function describeWhere(rule: Rule): string {
  const t = rule.enforce.find((x): x is AstGrepTarget => x.target === "ast-grep")!;
  const show = (c: WhereCondition): string => ("not" in c ? `not ${show(c.not)}` : JSON.stringify(c.type));
  return Object.entries(t.where ?? {}).map(([v, c]) => `$${v}: ${show(c)}`).join(", ");
}
