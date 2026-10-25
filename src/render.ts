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

export function checkTools(t: CheckTarget): string[] {
  const commands = [t.run, t.changed_run ?? "", t.fix_run ?? ""];
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

function scopeFunction(name: string, rule: Rule): string {
  const files = rule.scope.files ?? [];
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
}
export interface ActiveGrep {
  rule: Rule;
  target: GrepTarget;
}

/** The grep rules: forge_grep_meta_<n> (g_id g_mode g_pattern g_desc) and forge_grep_scope_<n>. */
function renderGreps(greps: ActiveGrep[]): string[] {
  const ids = (mode: GrepTarget["mode"]) => greps.filter((g) => g.target.mode === mode).map((g) => g.rule.id).join("|");
  const out = [`FORGE_NGREPS=${greps.length}`, `FORGE_GREP_NO_INCREASE='${ids("no-increase")}'`, `FORGE_GREP_NO_DECREASE='${ids("no-decrease")}'`];
  greps.forEach(({ rule, target }, i) => {
    const n = i + 1;
    out.push(
      "",
      `# grep ${n}. ${rule.id}`,
      `forge_grep_meta_${n}() {`,
      `  g_id=${shQuote(rule.id)}; g_mode=${target.mode}; g_flags=-${target.ignore_case ? "i" : ""}E`,
      `  g_pattern=${shQuote(target.pattern)}`,
      `  g_desc=${shQuote(rule.description)}`,
      "}",
      scopeFunction(`forge_grep_scope_${n}`, rule),
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
  checks.forEach(({ rule, target }, i) => {
    const n = i + 1;
    const tools = checkTools(target);
    const changed = target.changed_run !== undefined;
    out.push(
      "",
      `# ${n}. ${rule.id}`,
      `forge_check_meta_${n}() {`,
      `  c_id=${shQuote(rule.id)}; c_stage=${target.stage}; c_tools=${shQuote(tools.join(" "))}`,
      `  c_when=${target.when_changed ? 1 : 0}; c_localdb=${target.requires_local_db ? 1 : 0}; c_timeout=${target.timeout ?? "''"}`,
      `  c_changed=${changed ? 1 : 0}; c_files=${changed && target.changed_run!.includes("{files}") ? 1 : 0}; c_fix=${target.fix_run ? 1 : 0}`,
      "}",
      scopeFunction(`forge_scope_${n}`, rule),
      target.when_changed
        ? `forge_when_${n}() { case $1 in ${casePatterns(target.when_changed)}) return 0 ;; esac; return 1; }`
        : `forge_when_${n}() { return 1; }`,
      `forge_check_run_${n}() {`,
      target.run,
      "}",
    );
    if (changed) out.push(`forge_check_changed_${n}() {`, target.changed_run!.replaceAll("{files}", "$FORGE_FILES"), "}");
    if (target.fix_run) out.push(`forge_check_fix_${n}() {`, target.fix_run.replaceAll("{files}", "$FORGE_FILES"), "}");
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
];

/** forge_script_<name>() functions for the helper scripts the checks use. */
export function renderHelperScripts(checks: ActiveCheck[]): string {
  const out: string[] = [];
  for (const h of HELPER_SCRIPTS) {
    const used = new RegExp(`\\bforge_ratchet [a-z]+ ${h.name.replace(".", "\\.")}(\\s|$)`);
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
    "# Columns (tab-separated): id, decision, pattern (POSIX ERE on the JSON-escaped command), description.",
    `# Sources: ${sources.length ? sources.join(", ") : "none"}`,
  ];
  const ordered = [...guards.filter((g) => g.target.decision === "deny"), ...guards.filter((g) => g.target.decision === "ask")];
  for (const g of ordered) lines.push([g.rule.id, g.target.decision, g.target.pattern, g.rule.description].join("\t"));
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

export function renderAstGrepRule(rule: Rule, target: AstGrepTarget): string {
  const files = rule.scope.files ?? [];
  const include = files.filter((g) => !g.startsWith("!"));
  const ignores = files.filter((g) => g.startsWith("!")).map((g) => g.slice(1));
  const doc: Record<string, unknown> = {
    id: rule.id,
    language: astGrepLanguage(rule.scope.language),
    severity: "error",
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
          PreToolUse: [guard, { matcher: "Bash", hooks: [hook("bash-snapshot.sh", 15)] }],
          PermissionRequest: review,
          PostToolUse: [{ matcher: "Edit|Write|Bash", hooks: afterEdit }],
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
