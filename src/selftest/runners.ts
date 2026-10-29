import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import { delimiter, dirname, join } from "node:path";
import { examplePath } from "../audit.js";
import { globMatch } from "../core/globs.js";
import { checkTools, commandExists, PROJECT_TOOLS, PYTHON_TOOLS } from "../core/tools.js";
import { codeExamples } from "../examples.js";
import {
  astGrepRulePath,
  DEFAULT_ENGINE_SETTINGS,
  renderAstGrepRule,
  renderCheckScript,
  renderChecks,
  helperFiles,
  renderLib,
  renderSgConfig,
  typesLanguage,
  type TypesLanguage,
  wherePackages,
} from "../render/index.js";
import { AST_GREP_VERSION, ENGINE_PYTHON, HADOLINT_SHA256, HADOLINT_VERSION, type Job, ownedHash, run, runParallel, sha256 } from "../util.js";
import type { TestReport } from "../types.js";
import type { AstGrepTarget, CheckTarget, DepSpec, Example, ExampleFixture, Rule, WhereCondition } from "../types.js";

// ---------------------------------------------------------------- ast-grep

/**
 * The pinned ast-grep. Through bunx, the copy in bunx's cache (--no-install), installed
 * once when missing, as lib.sh's astgrep_bunx: `bunx -p` alone installs the package again
 * at every run, and concurrent installs fail now and then (a scan printing nothing).
 */
let bunxReady = false;
export function astGrepCommand(): string[] | undefined {
  if (run("bunx", ["--version"]).status === 0) {
    const pkg = `@ast-grep/cli@${AST_GREP_VERSION}`;
    if (!bunxReady && run("bunx", ["--no-install", "-p", pkg, "ast-grep", "--version"]).status !== 0) run("bunx", ["-p", pkg, "ast-grep", "--version"]);
    bunxReady = true;
    return ["bunx", "--no-install", "-p", pkg, "ast-grep"];
  }
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
export function guardJob(shell: string[], dir: string, command: string, cwd = "/"): Job {
  return { cmd: shell[0]!, args: [...shell.slice(1), join(dir, "guard.sh")], input: hookInput(command, cwd) };
}

/** guardDecision's reading of a guard.sh run. */
export function guardOutcome(r: { status: number; stdout: string; stderr: string }): string {
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
 * test runs, migrations, `astro check` and the lock-file checks, and the
 * large-file check (its files do not fit in a rule). Their examples are optional.
 */
export function checkExamplesOptional(t: CheckTarget): boolean {
  // forge_large_files: a file over its size limit does not fit in a rule file (a spec test runs it)
  // astro check and ngc: the framework's packages must be installed (a whole project)
  return t.stage === "test" || t.stage === "migration" || checkTools(t).some((x) => x === "astro" || x === "ngc" || x === "uv" || x === "bun") || /\bforge_large_files\b/.test(t.run);
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
const EXAMPLE_RUN = `g() ( export ${Object.entries(GIT_ENV)
  .map(([k, v]) => `${k}=${v}`)
  .join(" ")}; exec git -c maintenance.auto=false "$@" )
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
      const manifest =
        Object.entries(owned)
          .map(([p, text]) => `${ownedHash(p, text)}  ${p}`)
          .join("\n") + "\n";
      mkdirSync(repo);
      mkdirSync(stage);
      writeFiles(repo, { ".gitignore": "node_modules/\n.venv/\n", ...owned, ".forge/manifest.sha256": manifest, ...(fixture.head ?? {}) });
      writeFiles(stage, fixture.files);
      const path = new Set<string>();
      for (const tool of checkTools(t)) {
        if ((PROJECT_TOOLS as readonly string[]).includes(tool) || PYTHON_TOOLS.has(tool)) {
          const bin = toolBins.map((b) => join(b, tool)).find((p) => existsSync(p));
          if (!bin) {
            results[i] = { outcome: "skip", detail: `${tool} not available` };
            return;
          }
          const to = join(repo, PYTHON_TOOLS.has(tool) ? ".venv/bin" : "node_modules/.bin", tool);
          mkdirSync(dirname(to), { recursive: true });
          if (!existsSync(to)) symlinkSync(bin, to);
          // a node tool's companions installed with it (oxlint runs oxlint-tsgolint's tsgolint from node_modules/.bin)
          if (!PYTHON_TOOLS.has(tool))
            for (const c of COMPANIONS[tool] ?? []) if (existsSync(join(dirname(bin), c)) && !existsSync(join(dirname(to), c))) symlinkSync(join(dirname(bin), c), join(dirname(to), c));
        } else {
          // a system tool (§13.5): from the tool directories, else PATH; its directory goes first on the example's PATH
          const want = tool === "ty" ? "uvx" : tool;
          const dir = toolBins.find((b) => existsSync(join(b, want)));
          if (dir) path.add(dir);
          else if (!commandExists(want)) {
            results[i] = { outcome: "skip", detail: `${tool === "ty" ? "uvx (for ty)" : tool} not available` };
            return;
          }
        }
      }
      const mode = checkExampleMode(t);
      // its own TMPDIR: bunx installs ast-grep there at every run, and two at once in one place fail
      const tmp = join(dir, "tmp");
      mkdirSync(tmp);
      const env: Record<string, string> = { TMPDIR: `${tmp}/` };
      if (path.size) env.PATH = [...path, process.env.PATH ?? ""].join(delimiter);
      jobs.push({ cmd: "sh", args: ["-c", EXAMPLE_RUN, "sh", stage, ...(mode === "--turn" ? [mode, SNAP] : [mode])], cwd: repo, env });
      pending.push(i);
    });
    runParallel(jobs).forEach((r, j) => {
      const i = pending[j]!;
      const { rule, kind } = runs[i]!;
      const out = (r.stdout + r.stderr).trim();
      const skipped = new RegExp(`^forge:${rule.id.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")} skipped: (.*)$`, "m").exec(out);
      if (skipped) {
        results[i] = { outcome: "skip", detail: skipped[1]! };
        return;
      }
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

/** Binaries a project tool runs from the project's node_modules/.bin, installed next to it. */
const COMPANIONS: Record<string, string[]> = { oxlint: ["tsgolint"] };

/** Bin directories that hold the project tools for check examples: the project's, then PATH. */
export function defaultToolBins(repo?: string): string[] {
  return [...(repo ? [join(repo, "node_modules", ".bin"), join(repo, ".venv", "bin")] : []), ...(process.env.PATH ?? "").split(delimiter).filter(Boolean)];
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
  // the TypeScript project's compiler options are part of what makes it
  const tsconfig = {
    compilerOptions: {
      target: "ES2022",
      module: "ESNext",
      moduleResolution: "bundler",
      strict: true,
      noEmit: true,
      skipLibCheck: true,
      // global declarations (@types/node's, @types/bun's) are the ones a project lists: TypeScript 6 and later include none by default
      types: deps.filter((d) => d.name.startsWith("@types/")).map((d) => d.name.slice("@types/".length)),
    },
  };
  const key = sha256(JSON.stringify([lang, [...deps].sort((a, b) => (a.name < b.name ? -1 : 1)), lang === "typescript" ? [EXAMPLES_TYPESCRIPT, tsconfig] : ""])).slice(0, 16);
  const dir = join(examplesCache(), `${lang}-${key}`);
  const done = join(dir, ".forge-ready");
  if (existsSync(done)) return { dir };
  rmSync(dir, { recursive: true, force: true });
  mkdirSync(dir, { recursive: true });
  if (lang === "python") {
    writeFileSync(join(dir, "pyproject.toml"), `[project]\nname = "forge-examples"\nversion = "0"\nrequires-python = ">=${ENGINE_PYTHON}"\n`);
    let r = run("uv", ["venv", "-q", "--python", ENGINE_PYTHON, join(dir, ".venv")]);
    if (r.status === 0 && deps.length) r = run("uv", ["pip", "install", "-q", "--python", join(dir, ".venv", "bin", "python"), ...deps.map((d) => `${d.name}==${d.version}`)]);
    if (r.status !== 0) return { error: `uv: ${(r.stderr || r.stdout).trim().split("\n").slice(-3).join(" ")}` };
  } else {
    // the types engine is the project's TypeScript (§15.2): the examples get one unless deps name it
    const dependencies = { typescript: EXAMPLES_TYPESCRIPT, ...Object.fromEntries(deps.map((d) => [d.name, d.version])) };
    writeFileSync(join(dir, "package.json"), JSON.stringify({ name: "forge-examples", private: true, type: "module", dependencies }, null, 2));
    writeFileSync(join(dir, "tsconfig.json"), JSON.stringify(tsconfig, null, 2));
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
export function runTypesFilter(project: string, lang: TypesLanguage, matches: string, state: string, rules: Rule[] = []): { kept: string[]; output: string; status: number } {
  // the packages the rules ask about, as a project with them active would render (§15.2)
  const typesPackages = [...new Set(rules.flatMap((r) => r.enforce.filter((t): t is AstGrepTarget => t.target === "ast-grep").flatMap((t) => wherePackages(r, t))))].sort();
  const script = [
    renderLib(),
    "IFS='\n'",
    renderChecks([], [], [lang], undefined, "used", { ...DEFAULT_ENGINE_SETTINGS, typesPackages }),
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

export function typesExamples(rules: Rule[], report: TestReport): void {
  const { failures, notes } = report;
  const cmd = astGrepCommand();
  if (!cmd) return; // reported by the ast-grep tests
  const groups = new Map<string, { lang: TypesLanguage; deps: DepSpec[]; rules: Rule[] }>();
  for (const r of rules) {
    const t = r.enforce.find((x): x is AstGrepTarget => x.target === "ast-grep" && !!x.where)!;
    const lang = typesLanguage(r, t)!;
    const deps = r.examples?.deps ?? [];
    const key = JSON.stringify([lang, deps]);
    if (!groups.has(key)) groups.set(key, { lang, deps, rules: [] });
    groups.get(key)!.rules.push(r);
  }
  for (const { lang, deps, rules: group } of groups.values()) {
    const project = examplesProject(lang, deps);
    if (!project.dir) {
      failures.push(`types examples (${describeDeps(lang, deps)}): ${project.error}`);
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
        // the examples sit in the rule's scope under ex-*/, and the rule's files are scoped to that directory
        writeFileSync(join(sg, astGrepRulePath(r.id)), renderAstGrepRule(r, t, [rel]));
        for (const kind of ["match", "no_match"] as const)
          codeExamples(r, kind).forEach((code, i) => {
            const name = `${rel}/${examplePath(r, `${r.id.replaceAll("/", "_")}_${kind}_${i + 1}`)}`;
            mkdirSync(dirname(join(project.dir!, name)), { recursive: true });
            writeFileSync(join(project.dir!, name), code);
            expected.set(name, { rule: r, kind, i });
          });
      }
      const scan = run(cmd[0]!, [...cmd.slice(1), "scan", "-c", join(sg, "sgconfig.yml"), "--json=stream", "--include-metadata", ...expected.keys()], { cwd: project.dir });
      // ast-grep exits 0 without a match, 1 with an error-severity one; anything else is a failed scan
      if (scan.status > 1) {
        failures.push(`types examples (${lang}): ast-grep scan failed: ${(scan.stderr || scan.stdout).trim().split("\n").slice(0, 3).join(" | ")}`);
        continue;
      }
      const matches = join(state, "matches.json");
      writeFileSync(matches, scan.stdout);
      const res = runTypesFilter(project.dir, lang, matches, join(state, "s"), group);
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
      notes.push(`types (${lang}): ${expected.size} examples of ${group.length} rule(s) checked with ${describeDeps(lang, deps)}`);
    } finally {
      rmSync(sg, { recursive: true, force: true });
      rmSync(state, { recursive: true, force: true });
      rmSync(ex, { recursive: true, force: true });
    }
  }
}

function describeDeps(lang: TypesLanguage, deps: DepSpec[]): string {
  return deps.length ? deps.map((d) => `${d.name}@${d.version}`).join(", ") : lang === "python" ? "the standard library" : `TypeScript ${EXAMPLES_TYPESCRIPT}'s lib`;
}

function describeWhere(rule: Rule): string {
  const t = rule.enforce.find((x): x is AstGrepTarget => x.target === "ast-grep")!;
  const show = (c: WhereCondition): string =>
    "not" in c ? `not ${show(c.not)}` : "deprecated" in c ? "deprecated" : "declared" in c ? `declared ${JSON.stringify(c.declared)}` : JSON.stringify(c.type);
  return Object.entries(t.where ?? {})
    .map(([v, c]) => `$${v}: ${show(c)}`)
    .join(", ");
}
