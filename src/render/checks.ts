import { casePatterns, inDirs, scopeInDirs } from "../core/globs.js";
import { adapterUses } from "../adapters/index.js";
import { checkTools, commandsOf, PROJECT_TOOLS, RESOLVED_TOOLS } from "../core/tools.js";
import { defaultRoles, type Roles } from "../roles.js";
import { shQuote } from "../util.js";
import type { AstGrepTarget, CheckTarget, GrepTarget, Language, Rule } from "../types.js";
import { DEFAULT_ENGINE_SETTINGS, type EngineSettings, renderEngine, resolveEngine, type TypesLanguage } from "./engine.js";
import { renderRoles } from "./roles.js";
import { fill, renderLib, template } from "./template.js";

export const LANGUAGE_CASE: Record<Language, string[]> = {
  python: ["*.py", "*.pyi"],
  typescript: ["*.ts", "*.tsx", "*.mts", "*.cts", "*.js", "*.jsx", "*.mjs", "*.cjs"],
  tsx: ["*.tsx", "*.jsx"],
  yaml: ["*.yaml", "*.yml"],
  any: [],
};

/** forge_scope_<n> <path>: the rule's files; `config` (a lint check's config_files) are in scope too. */
export function scopeFunction(name: string, rule: Rule, dirs?: string[], config?: string[]): string {
  const files = scopeInDirs(rule.scope.files ?? [], dirs);
  const exclude = files.filter((g) => g.startsWith("!")).map((g) => g.slice(1));
  const include = files.filter((g) => !g.startsWith("!"));
  const lang = LANGUAGE_CASE[rule.scope.language];
  const lines = [`${name}() {`];
  const cfg = config ? scopeInDirs(config, dirs) : [];
  if (cfg.length) lines.push(`  case $1 in ${casePatterns(cfg)}) return 0 ;; esac`);
  if (exclude.length) lines.push(`  case $1 in ${casePatterns(exclude)}) return 1 ;; esac`);
  if (include.length) lines.push(`  case $1 in ${casePatterns(include)}) ;; *) return 1 ;; esac`);
  if (lang.length) lines.push(`  case $1 in ${lang.join("|")}) return 0 ;; esac`, "  return 1");
  else lines.push("  return 0");
  lines.push("}");
  return lines.join("\n");
}

export interface ActiveCheck {
  rule: Rule;
  target: CheckTarget;
  /** Workspace packages to run in (repository-relative); absent: the root. */
  dirs?: string[];
}

export interface ActiveAstGrep {
  rule: Rule;
  target: AstGrepTarget;
  /** Workspace packages its pack was detected in; absent: the whole repository. */
  dirs?: string[];
}

export interface ActiveGrep {
  rule: Rule;
  target: GrepTarget;
  /** Workspace packages its pack was detected in; absent: the whole repository. */
  dirs?: string[];
}

/** The grep rules: forge_grep_meta_<n> (g_id g_mode g_pattern g_desc) and forge_grep_scope_<n>. */
export function renderGreps(greps: ActiveGrep[]): string[] {
  const ids = (mode: GrepTarget["mode"]) =>
    greps
      .filter((g) => g.target.mode === mode)
      .map((g) => g.rule.id)
      .join("|");
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

/** The generated checks fragment (wrappers, per-check functions, grep rules, file roles, engine). */
/** The scripts that run the generated checks (and lib.sh, which they source): the engine modules they call are shipped (§8.1, §10.1). */
export const ENGINE_SCRIPTS = ["check.sh", "post-edit.sh", "typecheck.sh", "test-edit.sh", "session-start.sh", "lib.sh"];

/**
 * The generated checks fragment (wrappers, per-check functions, grep rules, file roles), then
 * the engine: the modules it and the scripts running it use ("used"), or every module ("all").
 */
export function renderChecks(
  checks: ActiveCheck[],
  greps: ActiveGrep[] = [],
  types: TypesLanguage[] = [],
  roles: Roles = defaultRoles(),
  engine: "used" | "all" = "used",
  settings: EngineSettings = DEFAULT_ENGINE_SETTINGS,
): string {
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
  // the operations the checks ask of adapted tools (§16.3: forge_tool_report)
  out.push(`FORGE_ADAPTER_USES=${shQuote(adapterUses(checks.flatMap(({ rule, target }) => commandsOf(target).map((command) => ({ rule: rule.id, command })))))}`);
  checks.forEach(({ rule, target, dirs }, i) => {
    const n = i + 1;
    const tools = checkTools(target);
    const changed = target.changed_run !== undefined;
    // in workspace packages: each command runs in every package through forge_in_dirs
    const body = (fn: string, cmd: string, form: "files" | "all") => (dirs ? [`${fn}() { forge_in_dirs ${fn}_dir ${form}; }`, `${fn}_dir() {`, cmd, "}"] : [`${fn}() {`, cmd, "}"]);
    const when = dirs ? target.when_changed?.flatMap((g) => inDirs(g, dirs)) : target.when_changed;
    out.push(
      "",
      `# ${n}. ${rule.id}`,
      `forge_check_meta_${n}() {`,
      `  c_id=${shQuote(rule.id)}; c_stage=${target.stage}; c_tools=${shQuote(tools.join(" "))}`,
      `  c_when=${target.when_changed ? 1 : 0}; c_localdb=${target.requires_local_db ? 1 : 0}; c_timeout=${target.timeout ?? "''"}`,
      `  c_changed=${changed ? 1 : 0}; c_files=${changed && target.changed_run!.includes("{files}") ? 1 : 0}; c_fix=${target.fix_run ? 1 : 0}; c_fixlint=${target.fix_lints ? 1 : 0}; c_fmt=${target.format_stdin ? 1 : 0}; c_rerun=${target.rerun ? 1 : 0}; c_warn=${target.fail_on_warnings ? 1 : 0}; c_dirs=${shQuote(dirs?.join(" ") ?? "")}`,
      "}",
      scopeFunction(`forge_scope_${n}`, rule, dirs, target.config_files),
      when ? `forge_when_${n}() { case $1 in ${casePatterns(when)}) return 0 ;; esac; return 1; }` : `forge_when_${n}() { return 1; }`,
      ...body(`forge_check_run_${n}`, target.run, "all"),
    );
    if (changed) out.push(...body(`forge_check_changed_${n}`, target.changed_run!.replaceAll("{files}", "$FORGE_FILES"), target.changed_run!.includes("{files}") ? "files" : "all"));
    if (target.fix_run) out.push(...body(`forge_check_fix_${n}`, target.fix_run.replaceAll("{files}", "$FORGE_FILES"), "files"));
    if (target.format_stdin) out.push(...formatFunction(n, target.format_stdin, dirs));
    if (target.rerun) out.push(...body(`forge_check_rerun_${n}`, target.rerun.replaceAll("{files}", "$FORGE_FILES"), "files"));
  });
  out.push("", ...renderGreps(greps), "", ...renderRoles(roles));
  out.push("# ---- end of generated checks");
  const modules = engine === "all" ? undefined : resolveEngine([out.join("\n"), ...ENGINE_SCRIPTS.map(template)]);
  out.push("", renderEngine(types, modules, settings));
  return out.join("\n");
}

/**
 * forge_check_fmt_<n>: the format_stdin filter on stdin, formatting as $FORGE_FILE. In
 * workspace packages it runs in the package holding the file (the deepest), with the
 * path relative to it, so the formatter reads that package's config, as the check does.
 */
export function formatFunction(n: number, cmd: string, dirs?: string[]): string[] {
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
  settings: EngineSettings = DEFAULT_ENGINE_SETTINGS,
): string {
  return fill(template("check.sh"), {
    LIB: renderLib().trimEnd(),
    CHECKS: renderChecks(checks, greps, types, roles, "used", settings),
    FORBID: ids.forbid,
    NO_INCREASE: ids.noIncrease,
    NO_DECREASE: ids.noDecrease,
  });
}
