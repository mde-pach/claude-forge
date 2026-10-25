import { readFileSync } from "node:fs";
import { join } from "node:path";
import { codeExamples } from "./packs.js";
import { toYaml } from "./yaml.js";
import { AST_GREP_VERSION, FORGE_VERSION, HADOLINT_SHA256, HADOLINT_VERSION, packageRoot, shQuote, SQUAWK_VERSION, ZIZMOR_VERSION } from "./util.js";
import type { AstGrepTarget, CheckTarget, GrepTarget, GuardTarget, Language, Level, Rule } from "./types.js";

export function template(name: string): string {
  return readFileSync(join(packageRoot(), "templates", "scripts", name), "utf8");
}

/** Replaces each @@KEY@@ once per occurrence; values are inserted literally. */
export function fill(text: string, values: Record<string, string>): string {
  return text.replace(/@@([A-Z_]+)@@/g, (m, key: string) => (key in values ? values[key]! : m));
}

export function renderLib(): string {
  return fill(template("lib.sh"), { AST_GREP_VERSION });
}

// ---------------------------------------------------------------- tools in commands

/** Project tools resolved through `tool <name>` (audited tools first). */
export const AUDITED_TOOLS = ["biome", "ruff", "basedpyright", "tsc"] as const;
export const PROJECT_TOOLS = [...AUDITED_TOOLS, "alembic", "pytest", "prisma", "vitest", "playwright", "astro", "python", "vue-tsc"] as const;
/** System runtimes, checked with `command -v`. */
export const SYSTEM_TOOLS = ["uv", "uvx", "bun", "docker"] as const;

/** Tools that engine helpers run, which their name in a command does not show. */
const HELPER_TOOLS: [RegExp, string][] = [
  [/\bforge_(zizmor|hadolint)\b/, "uvx"],
  [/\bforge_trusted_deps\b/, "bun"],
];

/** Tools invoked as commands in a shell string, in order of first use. */
export function toolsInCommand(cmd: string): string[] {
  const found: { tool: string; at: number }[] = [];
  for (const tool of [...PROJECT_TOOLS, ...SYSTEM_TOOLS]) {
    const m = new RegExp(`(^|[\\s;&|(\`]|\\$\\()${tool}(?=$|[\\s;&|)\`])`).exec(cmd);
    if (m) found.push({ tool, at: m.index });
  }
  return found.sort((a, b) => a.at - b.at).map((f) => f.tool);
}

/**
 * Project tools whose name has a "-" (no POSIX shell function can wrap them) that a
 * command runs directly instead of as `forge_exec_tool <name>`: run directly, they
 * would be whatever is on PATH, not the project's own.
 */
export function directDashedTools(cmd: string): string[] {
  const out = new Set<string>();
  for (const tool of PROJECT_TOOLS.filter((t) => t.includes("-"))) {
    const re = new RegExp(`(^|[\\s;&|(\`]|\\$\\()${tool.replace(/[.-]/g, "\\$&")}(?=$|[\\s;&|)\`])`, "g");
    for (const m of cmd.matchAll(re)) if (!/\bforge_exec_tool\s+$/.test(cmd.slice(0, m.index! + m[1]!.length))) out.add(tool);
  }
  return [...out];
}

export function checkTools(t: CheckTarget): string[] {
  const commands = [t.run, t.changed_run ?? "", t.fix_run ?? "", t.format_stdin ?? "", t.rerun ?? ""];
  const helpers = HELPER_TOOLS.filter(([re]) => commands.some((c) => re.test(c))).map(([, tool]) => tool);
  return [...new Set([...commands.flatMap(toolsInCommand), ...helpers])];
}

// ---------------------------------------------------------------- shell case patterns

const LANGUAGE_CASE: Record<Language, string[]> = {
  python: ["*.py", "*.pyi"],
  typescript: ["*.ts", "*.tsx", "*.mts", "*.cts", "*.js", "*.jsx", "*.mjs", "*.cjs"],
  tsx: ["*.tsx", "*.jsx"],
  yaml: ["*.yaml", "*.yml"],
  any: [],
};

function caseEscape(s: string): string {
  return s.replace(/[^A-Za-z0-9_./*?@+,=:%-]/g, "\\$&");
}

/** A glob as shell `case` patterns. In `case`, `*` also matches `/`. */
export function globToCase(glob: string): string[] {
  const brace = /\{([^{}]*)\}/.exec(glob);
  if (brace) {
    return brace[1]!
      .split(",")
      .flatMap((alt) => globToCase(glob.slice(0, brace.index) + alt + glob.slice(brace.index + brace[0].length)));
  }
  const out = new Set<string>();
  const variants = (g: string): string[] => {
    const i = g.indexOf("**/");
    if (i < 0) return [g.replace(/\*\*/g, "*")];
    const before = g.slice(0, i);
    const after = g.slice(i + 3);
    return [...variants(before + after), ...variants(before + "*/" + after)];
  };
  for (const v of variants(glob)) out.add(caseEscape(v.replace(/\*+/g, "*")));
  return [...out];
}

function casePatterns(globs: string[]): string {
  return [...new Set(globs.flatMap(globToCase))].join("|");
}

/**
 * A rule's scope as a shell function on repository paths. With dirs (workspace
 * packages), the scope's globs are relative to each package.
 */
/** A rule's scope globs, relative to each workspace package of dirs (the whole package when the rule has no include glob). */
export function scopeInDirs(files: string[], dirs?: string[]): string[] {
  if (!dirs) return files;
  const inDirs = (g: string) => dirs.map((d) => `${d}/${g}`);
  const include = files.filter((g) => !g.startsWith("!"));
  return [...(include.length ? include : ["**"]).flatMap(inDirs), ...files.filter((g) => g.startsWith("!")).flatMap((g) => inDirs(g.slice(1)).map((x) => "!" + x))];
}

function scopeFunction(name: string, rule: Rule, dirs?: string[]): string {
  const files = scopeInDirs(rule.scope.files ?? [], dirs);
  const exclude = files.filter((g) => g.startsWith("!")).map((g) => g.slice(1));
  const include = files.filter((g) => !g.startsWith("!"));
  const lang = LANGUAGE_CASE[rule.scope.language];
  const lines = [`${name}() {`];
  if (exclude.length) lines.push(`  case $1 in ${casePatterns(exclude)}) return 1 ;; esac`);
  if (include.length) lines.push(`  case $1 in ${casePatterns(include)}) ;; *) return 1 ;; esac`);
  if (lang.length) lines.push(`  case $1 in ${lang.join("|")}) return 0 ;; esac`, "  return 1");
  else lines.push("  return 0");
  lines.push("}");
  return lines.join("\n");
}

// ---------------------------------------------------------------- checks fragment

export interface ActiveCheck {
  rule: Rule;
  target: CheckTarget;
  /** Workspace packages to run in (repository-relative); absent: the root. */
  dirs?: string[];
}
export interface ActiveGrep {
  rule: Rule;
  target: GrepTarget;
  /** Workspace packages its pack was detected in; absent: the whole repository. */
  dirs?: string[];
}

/** The grep rules: forge_grep_meta_<n> (g_id g_mode g_pattern g_desc) and forge_grep_scope_<n>. */
function renderGreps(greps: ActiveGrep[]): string[] {
  const ids = (mode: GrepTarget["mode"]) => greps.filter((g) => g.target.mode === mode).map((g) => g.rule.id).join("|");
  const out = [`FORGE_NGREPS=${greps.length}`, `FORGE_GREP_NO_INCREASE='${ids("no-increase")}'`, `FORGE_GREP_NO_DECREASE='${ids("no-decrease")}'`];
  greps.forEach(({ rule, target, dirs }, i) => {
    const n = i + 1;
    out.push(
      "",
      `# grep ${n}. ${rule.id}`,
      `forge_grep_meta_${n}() {`,
      `  g_id=${shQuote(rule.id)}; g_mode=${target.mode}; g_flags=-${target.ignore_case ? "i" : ""}E`,
      `  g_pattern=${shQuote(target.pattern)}`,
      `  g_desc=${shQuote(rule.description)}`,
      "}",
      scopeFunction(`forge_grep_scope_${n}`, rule, dirs),
    );
  });
  return out;
}

/** The generated checks fragment (wrappers, per-check functions, grep rules, engine). */
export function renderChecks(checks: ActiveCheck[], greps: ActiveGrep[] = []): string {
  const used = new Set(checks.flatMap((c) => checkTools(c.target)));
  const out: string[] = ["# ---- generated checks"];
  // a POSIX shell function name has no "-": such tools are run as `forge_exec_tool <name>`
  const wrappers = PROJECT_TOOLS.filter((t) => used.has(t) && !t.includes("-"));
  if (wrappers.length) {
    out.push("# Project tools used in check commands, resolved through tool().");
    for (const t of wrappers) out.push(`${t}() { forge_exec_tool ${t} "$@"; }`);
  }
  out.push(`FORGE_NCHECKS=${checks.length}`);
  checks.forEach(({ rule, target, dirs }, i) => {
    const n = i + 1;
    const tools = checkTools(target);
    const changed = target.changed_run !== undefined;
    // in workspace packages: each command runs in every package through forge_in_dirs
    const body = (fn: string, cmd: string, form: "files" | "all") =>
      dirs ? [`${fn}() { forge_in_dirs ${fn}_dir ${form}; }`, `${fn}_dir() {`, cmd, "}"] : [`${fn}() {`, cmd, "}"];
    const when = dirs ? target.when_changed?.flatMap((g) => dirs.map((d) => `${d}/${g}`)) : target.when_changed;
    out.push(
      "",
      `# ${n}. ${rule.id}`,
      `forge_check_meta_${n}() {`,
      `  c_id=${shQuote(rule.id)}; c_stage=${target.stage}; c_tools=${shQuote(tools.join(" "))}`,
      `  c_when=${target.when_changed ? 1 : 0}; c_localdb=${target.requires_local_db ? 1 : 0}; c_timeout=${target.timeout ?? "''"}`,
      `  c_changed=${changed ? 1 : 0}; c_files=${changed && target.changed_run!.includes("{files}") ? 1 : 0}; c_fix=${target.fix_run ? 1 : 0}; c_fmt=${target.format_stdin ? 1 : 0}; c_rerun=${target.rerun ? 1 : 0}; c_dirs=${shQuote(dirs?.join(" ") ?? "")}`,
      "}",
      scopeFunction(`forge_scope_${n}`, rule, dirs),
      when
        ? `forge_when_${n}() { case $1 in ${casePatterns(when)}) return 0 ;; esac; return 1; }`
        : `forge_when_${n}() { return 1; }`,
      ...body(`forge_check_run_${n}`, target.run, "all"),
    );
    if (changed) out.push(...body(`forge_check_changed_${n}`, target.changed_run!.replaceAll("{files}", "$FORGE_FILES"), target.changed_run!.includes("{files}") ? "files" : "all"));
    if (target.fix_run) out.push(...body(`forge_check_fix_${n}`, target.fix_run.replaceAll("{files}", "$FORGE_FILES"), "files"));
    if (target.format_stdin) out.push(...formatFunction(n, target.format_stdin, dirs));
    if (target.rerun) out.push(...body(`forge_check_rerun_${n}`, target.rerun.replaceAll("{files}", "$FORGE_FILES"), "files"));
  });
  out.push("", ...renderGreps(greps));
  out.push("# ---- end of generated checks", "", fill(template("engine.sh"), {
      SQUAWK_VERSION,
      ZIZMOR_VERSION,
      HADOLINT_VERSION,
      HADOLINT_SHAS: Object.entries(HADOLINT_SHA256).map(([p, h]) => `${p} ${h}`).join("\n"),
    }).trimEnd());
  return out.join("\n");
}

/**
 * forge_check_fmt_<n>: the format_stdin filter on stdin, formatting as $FORGE_FILE. In
 * workspace packages it runs in the package holding the file (the deepest), with the
 * path relative to it, so the formatter reads that package's config, as the check does.
 */
function formatFunction(n: number, cmd: string, dirs?: string[]): string[] {
  const filter = cmd.replaceAll("{file}", '"$FORGE_FILE"');
  if (!dirs) return [`forge_check_fmt_${n}() {`, filter, "}"];
  const deepestFirst = [...dirs].sort((a, b) => b.split("/").length - a.split("/").length || (a < b ? -1 : 1));
  return [
    `forge_check_fmt_${n}() {`,
    `  for _ff_d in ${deepestFirst.map(shQuote).join(" ")}; do`,
    '    case $FORGE_FILE in "$_ff_d"/*)',
    '      ( FORGE_WS_ROOT=$(pwd); cd "$_ff_d" || exit 1; FORGE_FILE=${FORGE_FILE#"$_ff_d"/}',
    filter,
    "      )",
    "      return ;;",
    "    esac",
    "  done",
    filter,
    "}",
  ];
}

/**
 * test-edit.sh's rules: the checks running forge_test_edits (§10.5), as
 * forge_test_rule_<i>() { t_n (the check's number); t_id; t_review } and FORGE_NTESTRULES.
 */
export function renderTestRules(checks: ActiveCheck[]): string {
  const out: string[] = [];
  checks.forEach(({ rule, target }, i) => {
    if (target.run.trim() !== "forge_test_edits") return;
    out.push(`forge_test_rule_${out.length + 1}() { t_n=${i + 1}; t_id=${shQuote(rule.id)}; t_review=${shQuote(rule.review ?? "")}; }`);
  });
  return [`FORGE_NTESTRULES=${out.length}`, ...out].join("\n");
}

/** .forge/check.sh: the library, the checks fragment and the ast-grep rule ids by mode. */
export function renderCheckScript(checks: ActiveCheck[], ids: { forbid: string; noIncrease: string; noDecrease: string }, greps: ActiveGrep[] = []): string {
  return fill(template("check.sh"), {
    LIB: renderLib().trimEnd(),
    CHECKS: renderChecks(checks, greps),
    SCRIPTS: renderHelperScripts(checks),
    FORBID: ids.forbid,
    NO_INCREASE: ids.noIncrease,
    NO_DECREASE: ids.noDecrease,
  });
}

/** Tools resolved from the project's .venv/bin (the others from node_modules/.bin). */
export const PYTHON_TOOLS = new Set(["ruff", "basedpyright", "alembic", "pytest", "python"]);

/**
 * Scripts that engine helpers run with the project's own runtime. They are
 * embedded in check.sh as here-documents (forge_script_<name>), only when an
 * active check uses them: as separate files, the project's formatters would
 * rewrite them (ruff format ., biome check --write .).
 */
const HELPER_SCRIPTS: { name: string; template: string }[] = [
  { name: "ratchet.py", template: "ratchet.py" },
  { name: "ratchet.mjs", template: "ratchet.mjs" },
  { name: "drf_permissions.py", template: "drf_permissions.py" },
  { name: "public_api.py", template: "public_api.py" },
  { name: "public_api.mjs", template: "public_api.mjs" },
];

/** forge_script_<name>() functions for the helper scripts the checks use. */
export function renderHelperScripts(checks: ActiveCheck[]): string {
  const out: string[] = [];
  for (const h of HELPER_SCRIPTS) {
    const used = new RegExp(`\\bforge_(ratchet|helper) [a-z]+ ${h.name.replace(".", "\\.")}(\\s|$)`);
    if (!checks.some((c) => used.test(c.target.run) || used.test(c.target.changed_run ?? ""))) continue;
    const text = readFileSync(join(packageRoot(), "templates", "helpers", h.template), "utf8");
    if (text.split("\n").includes("FORGE_SCRIPT_END")) throw new Error(`${h.template}: contains the here-document delimiter`);
    out.push(`forge_script_${h.name.replace(/\W/g, "_")}() {`, "cat <<'FORGE_SCRIPT_END'", text.trimEnd(), "FORGE_SCRIPT_END", "}");
  }
  return out.join("\n");
}

// ---------------------------------------------------------------- guard.rules

export interface ActiveGuard {
  rule: Rule;
  target: GuardTarget;
}

export function ruleSource(rule: Rule, base: string): string {
  if (rule.origin === "local") return rule.file.startsWith(base + "/") ? rule.file.slice(base.length + 1) : rule.file;
  return `packs/${rule.pack}/rules/${rule.file.split("/").pop()}`;
}

export function renderGuardRules(guards: ActiveGuard[], base: string): string {
  const sources = [...new Set(guards.map((g) => ruleSource(g.rule, base)))].sort();
  const lines = [
    "# Generated by forge. Do not edit; changes are detected by the integrity check.",
    "# Columns (tab-separated): id, decision, pattern (POSIX ERE on the JSON-escaped command), description, check (a guard.sh check deciding when the pattern matches), review (the question a reviewer answers).",
    `# Sources: ${sources.length ? sources.join(", ") : "none"}`,
  ];
  const ordered = [...guards.filter((g) => g.target.decision === "deny"), ...guards.filter((g) => g.target.decision === "ask")];
  for (const g of ordered) {
    const cols = [g.rule.id, g.target.decision, g.target.pattern, g.rule.description];
    if (g.target.check || g.rule.review) cols.push(g.target.check ?? "");
    if (g.rule.review) cols.push(g.rule.review);
    lines.push(cols.join("\t"));
  }
  return lines.join("\n") + "\n";
}

// ---------------------------------------------------------------- ast-grep

export function astGrepLanguage(lang: Language): string {
  if (lang === "python") return "Python";
  if (lang === "typescript" || lang === "tsx") return "Tsx";
  if (lang === "yaml") return "Yaml";
  throw new Error(`ast-grep targets need a concrete scope.language, not "${lang}"`);
}

export function astGrepRulePath(id: string): string {
  return `rules/${id}.yml`;
}
export function astGrepTestPath(id: string): string {
  return `rule-tests/${id.replaceAll("/", "-")}-test.yml`;
}

/** An ast-grep rule file; with dirs (the workspace packages its pack was detected in), its globs are relative to them. */
export function renderAstGrepRule(rule: Rule, target: AstGrepTarget, dirs?: string[]): string {
  const files = scopeInDirs(rule.scope.files ?? [], dirs);
  const include = files.filter((g) => !g.startsWith("!"));
  const ignores = files.filter((g) => g.startsWith("!")).map((g) => g.slice(1));
  const doc: Record<string, unknown> = {
    id: rule.id,
    language: astGrepLanguage(rule.scope.language),
    severity: target.mode === "advise" ? "warning" : "error",
    message: `forge:${rule.id} ${rule.description}`,
  };
  if (include.length) doc.files = include;
  if (ignores.length) doc.ignores = ignores;
  doc.rule = target.rule;
  if (target.constraints) doc.constraints = target.constraints;
  if (target.utils) doc.utils = target.utils;
  return `# Generated by forge from ${rule.id} (mode: ${target.mode}).\n${toYaml(doc)}`;
}

export function renderAstGrepTest(rule: Rule): string {
  return toYaml({ id: rule.id, valid: codeExamples(rule, "no_match"), invalid: codeExamples(rule, "match") });
}

export function renderSgConfig(): string {
  return [
    "# Generated by forge.",
    "ruleDirs: [rules]",
    "testConfigs:",
    "  - testDir: rule-tests",
    "languageGlobs:",
    '  tsx: ["*.ts", "*.mts", "*.cts"]',
    "",
  ].join("\n");
}

// ---------------------------------------------------------------- plugin

function hook(script: string, timeout: number, extra: Record<string, unknown> = {}) {
  return { type: "command", command: `sh "\${CLAUDE_PLUGIN_ROOT}/scripts/${script}"`, timeout, ...extra };
}

export function renderHooks(level: Level): string {
  // 180 s: an ask nobody can answer waits for an independent review (review.sh); the usual answer takes milliseconds
  const guard = { matcher: "Bash", hooks: [hook("guard.sh", 180, { onFailure: "block" })] };
  const review = [{ hooks: [hook("review.sh", 180)] }];
  const afterEdit = [hook("post-edit.sh", 60), hook("typecheck.sh", 300, { asyncRewake: true })];
  // an edit changing a test that existed at turn start asks (reviewed when nobody can answer); once it ran, it was approved
  const testEdit = (timeout: number) => ({ matcher: "Edit|Write", hooks: [hook("test-edit.sh", timeout)] });
  const hooks =
    level === "user"
      ? {
          PreToolUse: [guard],
          PermissionRequest: review,
          Notification: [{ matcher: "permission_prompt|idle_prompt", hooks: [hook("notify.sh", 5)] }],
        }
      : {
          SessionStart: [{ matcher: "startup|resume|clear|compact", hooks: [hook("session-start.sh", 10)] }],
          UserPromptSubmit: [{ hooks: [hook("turn-start.sh", 10)] }],
          PreToolUse: [guard, { matcher: "Bash", hooks: [hook("bash-snapshot.sh", 15)] }, testEdit(180)],
          PermissionRequest: review,
          PostToolUse: [{ matcher: "Edit|Write|Bash", hooks: afterEdit }, testEdit(10)],
          // a Bash command that fails (or is interrupted) partway may already have changed files; a failed Edit or Write changes nothing
          PostToolUseFailure: [{ matcher: "Bash", hooks: afterEdit }],
          Stop: [{ hooks: [hook("stop.sh", 600)] }],
        };
  return JSON.stringify({ description: "Generated by forge. Do not edit; run forge apply.", hooks }, null, 2) + "\n";
}

export function renderPlugin(level: Level): string {
  const name = level === "user" ? "forge-user" : "forge-project";
  return (
    JSON.stringify(
      { name, version: FORGE_VERSION, description: "Generated by forge. Do not edit; run forge apply to regenerate." },
      null,
      2,
    ) + "\n"
  );
}
