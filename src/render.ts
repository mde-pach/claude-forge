import { readFileSync } from "node:fs";
import { join } from "node:path";
import { toYaml } from "./yaml.js";
import { AST_GREP_VERSION, FORGE_VERSION, packageRoot, shQuote, SQUAWK_VERSION } from "./util.js";
import type { AstGrepTarget, CheckTarget, GuardTarget, Language, Level, Rule } from "./types.js";

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
export const PROJECT_TOOLS = [...AUDITED_TOOLS, "alembic", "pytest", "prisma", "vitest", "playwright", "astro"] as const;
/** System runtimes, checked with `command -v`. */
export const SYSTEM_TOOLS = ["uv", "bun"] as const;

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
  return [...new Set([...toolsInCommand(t.run), ...toolsInCommand(t.changed_run ?? "")])];
}

// ---------------------------------------------------------------- shell case patterns

const LANGUAGE_CASE: Record<Language, string[]> = {
  python: ["*.py", "*.pyi"],
  typescript: ["*.ts", "*.tsx", "*.mts", "*.cts", "*.js", "*.jsx", "*.mjs", "*.cjs"],
  tsx: ["*.tsx", "*.jsx"],
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

function scopeFunction(n: number, rule: Rule): string {
  const files = rule.scope.files ?? [];
  const exclude = files.filter((g) => g.startsWith("!")).map((g) => g.slice(1));
  const include = files.filter((g) => !g.startsWith("!"));
  const lang = LANGUAGE_CASE[rule.scope.language];
  const lines = [`forge_scope_${n}() {`];
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

/** The generated checks fragment (wrappers, per-check functions, engine). */
export function renderChecks(checks: ActiveCheck[]): string {
  const used = new Set(checks.flatMap((c) => checkTools(c.target)));
  const out: string[] = ["# ---- generated checks"];
  const wrappers = PROJECT_TOOLS.filter((t) => used.has(t));
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
      `  c_changed=${changed ? 1 : 0}; c_files=${changed && target.changed_run!.includes("{files}") ? 1 : 0}`,
      "}",
      scopeFunction(n, rule),
      target.when_changed
        ? `forge_when_${n}() { case $1 in ${casePatterns(target.when_changed)}) return 0 ;; esac; return 1; }`
        : `forge_when_${n}() { return 1; }`,
      `forge_check_run_${n}() {`,
      target.run,
      "}",
    );
    if (changed) out.push(`forge_check_changed_${n}() {`, target.changed_run!.replaceAll("{files}", "$FORGE_FILES"), "}");
  });
  out.push("# ---- end of generated checks", "", fill(template("engine.sh"), { SQUAWK_VERSION }).trimEnd());
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
  return toYaml({ id: rule.id, valid: rule.examples?.no_match ?? [], invalid: rule.examples?.match ?? [] });
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
  const guard = { matcher: "Bash", hooks: [hook("guard.sh", 5, { onFailure: "block" })] };
  const hooks =
    level === "user"
      ? {
          PreToolUse: [guard],
          Notification: [{ matcher: "permission_prompt|idle_prompt", hooks: [hook("notify.sh", 5)] }],
        }
      : {
          SessionStart: [{ matcher: "startup|resume|clear|compact", hooks: [hook("session-start.sh", 10)] }],
          UserPromptSubmit: [{ hooks: [hook("turn-start.sh", 10)] }],
          PreToolUse: [guard],
          PostToolUse: [
            { matcher: "Edit|Write", hooks: [hook("post-edit.sh", 30), hook("typecheck.sh", 300, { asyncRewake: true })] },
          ],
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
