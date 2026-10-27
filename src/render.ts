import { readFileSync } from "node:fs";
import { join } from "node:path";
import { codeExamples } from "./packs.js";
import { defaultRoles, type Roles } from "./roles.js";
import { toYaml } from "./yaml.js";
import { AST_GREP_VERSION, ENGINE_PYTHON, FORGE_VERSION, HADOLINT_SHA256, HADOLINT_VERSION, packageRoot, releaseTable, sha256, shQuote, SQUAWK_VERSION, TFLINT_RULESETS, TY_VERSION, ZIZMOR_VERSION } from "./util.js";
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
export const AUDITED_TOOLS = ["biome", "ruff", "ty", "tsc"] as const;
export const PROJECT_TOOLS = ["biome", "ruff", "tsc", "alembic", "pytest", "prisma", "vitest", "playwright", "astro", "python", "vue-tsc", "ngc", "mikro-orm", "graphql-codegen", "wrangler"] as const;
/** Forge's own pinned tools used in check commands: ty (Python's type checker, through uvx, §15.2). */
export const FORGE_TOOLS = ["ty"] as const;
/**
 * System tools, found on PATH: runtimes, package managers, and infrastructure CLIs a
 * project installs outside node_modules and .venv. A check names terraform, kustomize:
 * the engine runs the project's own (tofu in an OpenTofu project; `kubectl kustomize`
 * without kustomize). yarn counts only as Yarn 2 or later (Berry), the Yarn the
 * typescript/yarn pack is for.
 */
export const SYSTEM_TOOLS = ["uv", "uvx", "bun", "npm", "pnpm", "yarn", "docker", "terraform", "tofu", "helm", "kustomize", "kubectl"] as const;
/** System tools a check runs through an engine function that picks the binary (forge_terraform, forge_kustomize). */
const RESOLVED_TOOLS = ["terraform", "kustomize"] as const;

/** Tools that engine helpers run, which their name in a command does not show. */
const HELPER_TOOLS: [RegExp, string][] = [
  [/\bforge_(zizmor|hadolint)\b/, "uvx"],
  [/\bforge_trusted_deps\b/, "bun"],
  [/\bforge_tf_validate\b/, "terraform"],
  [/\bforge_helm\b/, "helm"],
];

/** Tools invoked as commands in a shell string, in order of first use. */
export function toolsInCommand(cmd: string): string[] {
  const found: { tool: string; at: number }[] = [];
  for (const tool of [...PROJECT_TOOLS, ...FORGE_TOOLS, ...SYSTEM_TOOLS]) {
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
  // bracket expressions ([0-9], [!x]) are kept: case patterns have them too
  return s
    .split(/(\[!?[^\]/'"\\]+\])/)
    .map((part, i) => (i % 2 ? part : part.replace(/[^A-Za-z0-9_./*?@+,=:%-]/g, "\\$&")))
    .join("");
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

/** The languages of the types engine (§15.2). */
export type TypesLanguage = "python" | "typescript";

/** The types engine a rule's ast-grep target needs (§15.2): its language's, when it has a where. */
export function typesLanguage(rule: Rule, target: AstGrepTarget): TypesLanguage | undefined {
  if (!target.where) return undefined;
  return rule.scope.language === "python" ? "python" : "typescript";
}

/** A hash of the engine adapters: a daemon started by another revision is restarted. */
function typesRevision(): string {
  const text = ["types_python.py", "types_typescript.mjs"].map((f) => readFileSync(join(packageRoot(), "templates", "helpers", f), "utf8")).join("\0");
  return sha256(text).slice(0, 12);
}

/** The file roles engine helpers name themselves (forge_dockerfiles: @dockerfile, forge_alembic_files: @alembic-migrations): always rendered. */
export const ENGINE_ROLES = ["dockerfile", "alembic-migrations"];

/**
 * forge_role <@role> <path> (§15.4): succeeds when the repository path is one of the role's
 * files, by the effective patterns rendered literally (exclusions first).
 */
export function renderRoles(roles: Roles): string[] {
  const out = ["# File roles (§15.4): forge_role <@role> <path>.", "forge_role() {", "  case ${1#@} in"];
  for (const [name, role] of [...roles.entries()].sort(([a], [b]) => (a < b ? -1 : 1))) {
    const inc = casePatterns(role.patterns.filter((p) => !p.exclude).map((p) => p.glob));
    const exc = casePatterns(role.patterns.filter((p) => p.exclude).map((p) => p.glob));
    const body = [exc ? `case $2 in ${exc}) return 1 ;; esac` : "", inc ? `case $2 in ${inc}) return 0 ;; esac` : ""].filter(Boolean);
    out.push(`    ${name}) ${body.length ? body.join("; ") + " " : ""};;`);
  }
  out.push("  esac", "  return 1", "}");
  return out;
}

/** The generated checks fragment (wrappers, per-check functions, grep rules, file roles, engine). */
export function renderChecks(checks: ActiveCheck[], greps: ActiveGrep[] = [], types: TypesLanguage[] = [], roles: Roles = defaultRoles()): string {
  const used = new Set(checks.flatMap((c) => checkTools(c.target)));
  const out: string[] = ["# ---- generated checks"];
  // a POSIX shell function name has no "-": such tools are run as `forge_exec_tool <name>`
  const wrappers = PROJECT_TOOLS.filter((t) => used.has(t) && !t.includes("-"));
  if (wrappers.length) {
    out.push("# Project tools used in check commands, resolved through tool().");
    for (const t of wrappers) out.push(`${t}() { forge_exec_tool ${t} "$@"; }`);
  }
  if (used.has("ty")) out.push("# ty is Forge's own, pinned (§15.2).", 'ty() { forge_ty "$@"; }');
  const resolved = RESOLVED_TOOLS.filter((t) => used.has(t));
  if (resolved.length) {
    out.push("# System tools whose binary the engine picks (forge_has_tool).");
    for (const t of resolved) out.push(`${t}() { forge_${t} "$@"; }`);
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
      `  c_changed=${changed ? 1 : 0}; c_files=${changed && target.changed_run!.includes("{files}") ? 1 : 0}; c_fix=${target.fix_run ? 1 : 0}; c_fmt=${target.format_stdin ? 1 : 0}; c_rerun=${target.rerun ? 1 : 0}; c_warn=${target.fail_on_warnings ? 1 : 0}; c_dirs=${shQuote(dirs?.join(" ") ?? "")}`,
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
  out.push("", ...renderGreps(greps), "", ...renderRoles(roles));
  out.push("# ---- end of generated checks", "", fill(template("engine.sh"), {
      SQUAWK_VERSION,
      ZIZMOR_VERSION,
      HADOLINT_VERSION,
      HADOLINT_SHAS: Object.entries(HADOLINT_SHA256).map(([p, h]) => `${p} ${h}`).join("\n"),
      RELEASES: releaseTable(),
      TFLINT_RULESETS: Object.entries(TFLINT_RULESETS).map(([p, v]) => `${p} ${v}`).join("\n"),
      // newline-separated: IFS is a newline in the generated scripts
      TYPES_LANGS: types.join("\n"),
      TY_VERSION,
      ENGINE_PYTHON,
      TYPES_REV: types.length ? typesRevision() : "",
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
export function renderCheckScript(
  checks: ActiveCheck[],
  ids: { forbid: string; noIncrease: string; noDecrease: string },
  greps: ActiveGrep[] = [],
  types: TypesLanguage[] = [],
  roles: Roles = defaultRoles(),
): string {
  return fill(template("check.sh"), {
    LIB: renderLib().trimEnd(),
    CHECKS: renderChecks(checks, greps, types, roles),
    FORBID: ids.forbid,
    NO_INCREASE: ids.noIncrease,
    NO_DECREASE: ids.noDecrease,
  });
}

/** Tools resolved from the project's .venv/bin (the others from node_modules/.bin). */
export const PYTHON_TOOLS = new Set(["ruff", "alembic", "pytest", "python"]);

/**
 * Scripts that engine helpers and the types engine run with a runtime (§13.4, §15.2),
 * shipped as plain files in .forge/helpers/, only when an active check or rule uses
 * them. A project formatter may rewrite them: their integrity hash ignores formatting
 * (ownedHash).
 */
const HELPER_SCRIPTS = ["ratchet.py", "ratchet.mjs", "drf_permissions.py", "public_api.py", "public_api.mjs", "fk_index.py"];
export const HELPERS_DIR = ".forge/helpers";

/** The helper files the checks and types engines use: path (repository-relative) -> content. */
export function helperFiles(checks: ActiveCheck[], types: TypesLanguage[] = []): Map<string, string> {
  const out = new Map<string, string>();
  const read = (name: string) => readFileSync(join(packageRoot(), "templates", "helpers", name), "utf8");
  for (const name of HELPER_SCRIPTS) {
    const used = new RegExp(`\\bforge_(ratchet|helper) [a-z]+ ${name.replace(".", "\\.")}(\\s|$)`);
    if (checks.some((c) => used.test(c.target.run) || used.test(c.target.changed_run ?? ""))) out.set(`${HELPERS_DIR}/${name}`, read(name));
  }
  for (const lang of types) {
    const name = lang === "python" ? "types_python.py" : "types_typescript.mjs";
    out.set(`${HELPERS_DIR}/${name}`, read(name));
  }
  return out;
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
  // engine capabilities travel with each match (--include-metadata) to forge_types_filter
  if (target.where) doc.metadata = { forge_where: target.where };
  return `# Generated by forge from ${rule.id} (mode: ${target.mode}).\n${toYaml(doc)}`;
}

/**
 * The ast-grep test of a rule's examples. With a where (§15.1), ast-grep alone only
 * proves that every match example matches; the no_match examples need the types engine
 * (selftest's types examples).
 */
export function renderAstGrepTest(rule: Rule): string {
  const typed = rule.enforce.some((t) => t.target === "ast-grep" && t.where);
  return toYaml({ id: rule.id, valid: typed ? [] : codeExamples(rule, "no_match"), invalid: codeExamples(rule, "match") });
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

export function renderHooks(level: Level, opts: { types?: boolean } = {}): string {
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
          // the last session to end stops the types engines (§15.3)
          ...(opts.types ? { SessionEnd: [{ hooks: [hook("session-end.sh", 10)] }] } : {}),
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
