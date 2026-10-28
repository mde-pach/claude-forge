import { globToRegExp } from "../core/globs.js";
import { commandExists, TY_COMMAND } from "../core/tools.js";
import { renderLib } from "../render/index.js";
import { kindOf, targetsByPhase } from "../targets/index.js";
import type { AuditResult } from "../audit.js";
import { roleGlobs } from "../roles.js";
import { mergeTomlSnippets } from "../snippets.js";
import { TY_VERSION, listRepoFiles, run, runScript } from "../util.js";
import { roleSummary } from "./changes.js";
import type { Ctx } from "./context.js";

/**
 * Fetches the pinned Python types engine into the uvx cache (§15.2), so the hooks start
 * it offline; TypeScript's is the project's own (node_modules). Returns what could not be
 * provisioned.
 */
export function provisionEngines(langs: string[]): string[] {
  if (!langs.includes("python")) return [];
  const r = run(TY_COMMAND[0]!, [...TY_COMMAND.slice(1), "--version"]);
  if (r.status === 0) return [];
  return [
    `forge: the python types engine (ty ${TY_VERSION}) could not be provisioned with uvx: ${(r.stderr || r.stdout).trim().split("\n").slice(-1)[0] || `exit ${r.status}`}; rules with type constraints are reported unchecked until it is`,
  ];
}

/**
 * When the setup enables Claude Code's sandbox on Linux, whether bubblewrap can
 * start it here: a container with Docker's default seccomp/AppArmor profile cannot
 * create namespaces ("Operation not permitted"), and one with a masked /proc cannot
 * mount a new proc, which Claude Code's sandbox does (so the probe passes --proc).
 * Returns the message to print, or undefined when the sandbox can run (or is not
 * Linux's to check).
 */
export function sandboxProblem(ctx: Ctx): string | undefined {
  if (process.platform !== "linux") return undefined;
  if (!ctx.plan.settings.some((e) => e.path === "sandbox.enabled" && e.value === true)) return undefined;
  const off = 'or turn the sandbox off knowingly: set "sandbox": {"enabled": false} in ~/.claude/settings.json (forge keeps a value you set)';
  const tools = ["bwrap", "socat"].filter((c) => !commandExists(c));
  if (tools.length) return `forge: sandbox: Claude Code's sandbox needs ${tools.join(" and ")} on Linux (apt install bubblewrap socat, or your distribution's packages), ${off}.`;
  const r = run("bwrap", ["--ro-bind", "/", "/", "--dev", "/dev", "--proc", "/proc", "--unshare-all", "--die-with-parent", "true"]);
  if (r.status === 0) return undefined;
  const why = (r.stderr || r.stdout).trim().split("\n")[0] || `exit ${r.status}`;
  return `forge: sandbox: Claude Code's sandbox cannot start here: bubblewrap failed (${why}). In a container with Docker's default seccomp/AppArmor profile, namespaces are not allowed and /proc is masked. Run the container with what bubblewrap needs (e.g. --security-opt seccomp=unconfined --security-opt apparmor=unconfined --security-opt systempaths=unconfined, or --cap-add SYS_ADMIN), ${off}.`;
}

/**
 * One config block per TOML tool (ruff, ty) and audited place for its unsatisfied audits,
 * when there are several: rules add entries to the same tables (extend-select,
 * banned-api), and a table written twice is not valid TOML. Keyed "<tool>" or "<tool> (in <dir>)".
 */
export function mergedSnippets(audits: AuditResult[]): Record<string, string> {
  const groups = new Map<string, string[]>();
  for (const a of audits)
    if (a.status === "unsatisfied" && a.snippet && (a.tool === "ruff" || a.tool === "ty")) {
      const k = `${a.tool}${a.dir ? ` (in ${a.dir})` : ""}`;
      if (!groups.has(k)) groups.set(k, []);
      if (!groups.get(k)!.includes(a.snippet)) groups.get(k)!.push(a.snippet);
    }
  const out: Record<string, string> = {};
  for (const [k, list] of groups) {
    const merged = list.length > 1 ? mergeTomlSnippets(list) : undefined;
    if (merged) out[k] = merged;
  }
  return out;
}

export function formatMerged(audits: AuditResult[]): string[] {
  return Object.entries(mergedSnippets(audits)).flatMap(([k, block]) => [
    `forge: to satisfy every unsatisfied ${k} audit, add to the ${k.split(" ")[0]} config:`,
    ...block.split("\n").map((l) => `  ${l}`),
  ]);
}

export function formatAudit(a: AuditResult): string {
  const head = `forge:${a.rule} ${a.tool} ${a.expects}${a.dir ? ` (in ${a.dir})` : ""}: ${a.status}`;
  if (a.status === "satisfied") return head;
  const lines = [head + (a.detail ? ` (${a.detail})` : "")];
  if (a.snippet) lines.push(`  add to the ${a.tool} config: ${a.snippet}`);
  return lines.join("\n");
}

/**
 * Tools the setup needs that are missing. Project level: the generated scripts'
 * own forge_missing_tools (what SessionStart reports), run here on the checks
 * fragment, so both always agree.
 */
export function missingTools(ctx: Ctx): string[] {
  if (ctx.level === "user" || !ctx.plan.checksFragment) {
    return !commandExists("bunx") && !commandExists("uvx") ? ["bunx or uvx (ast-grep)"] : [];
  }
  const script = `${renderLib()}\nIFS='\n'\n${ctx.plan.checksFragment}\nforge_missing_tools\n`;
  const r = runScript(script, { cwd: ctx.base });
  return r.stdout.trim() ? r.stdout.trim().split(", ") : [];
}

/** The remote's default branch (origin/HEAD), else "main". */
export function defaultBranch(base: string): string {
  const r = run("git", ["symbolic-ref", "--quiet", "--short", "refs/remotes/origin/HEAD"], { cwd: base });
  return r.status === 0 && r.stdout.trim().startsWith("origin/") ? r.stdout.trim().slice("origin/".length) : "main";
}

export interface Enforcement {
  id: string;
  enforced: boolean;
  /** What enforces it here. */
  by: string[];
  /** Why a target does not enforce it here; the first one is the rule's summary reason. */
  not_enforced: string[];
}

export const UNSATISFIED: Record<string, string> = {
  ruff: "ruff codes not selected",
  biome: "Biome rules not at error",
  ty: "ty rules not at error",
  tsc: "tsc options not set",
  eslint: "ESLint rules not at error",
};

/** Whether each active rule is actually enforced in this project, and why not. */
export function enforcement(ctx: Ctx, audits: AuditResult[], missing: string[]): Enforcement[] {
  // "tsc (packages/app)": missing in a workspace package
  const absent = new Set(missing.map((m) => m.replace(/ \(.*\)$/, "")));
  return ctx.plan.active.map((st) => {
    const by: string[] = [];
    const not: string[] = [];
    // audited in several packages: enforced by the tool only where every package's config enforces it
    const mine = audits.filter((x) => x.rule === st.rule.id);
    for (const tool of new Set(mine.map((a) => a.tool))) {
      const each = mine.filter((a) => a.tool === tool);
      const bad = each.find((a) => a.status !== "satisfied");
      if (!bad) by.push(`${tool} ${each[0]!.expects}`);
      else if (bad.status === "unsatisfied") not.push(UNSATISFIED[tool]!);
      else not.push(/not installed/.test(bad.detail ?? "") ? `${tool} not installed` : `${tool} audit skipped`);
    }
    for (const n of st.notEnforced) not.push(n.replace(/^not enforced here: /, ""));
    for (const t of targetsByPhase(st.rule)) {
      const e = kindOf(t).enforcement(st, t, absent);
      if (e.by) by.push(e.by);
      if (e.not) not.push(e.not);
    }
    return { id: st.rule.id, enforced: by.length > 0, by: [...new Set(by)], not_enforced: [...new Set(not)] };
  });
}

export function enforcementSummary(rules: Enforcement[]): string {
  const off = rules.filter((r) => !r.enforced);
  const reasons = new Map<string, number>();
  for (const r of off) {
    const why = r.not_enforced[0] ?? "nothing applies here";
    reasons.set(why, (reasons.get(why) ?? 0) + 1);
  }
  const detail = [...reasons.entries()]
    .sort((a, b) => b[1] - a[1])
    .map(([w, n]) => `${n} ${w}`)
    .join(", ");
  return `${rules.length} active, ${rules.length - off.length} enforced, ${off.length} not enforced${off.length ? ` (${detail})` : ""}`;
}

/**
 * The file roles the active rules name (§15.4): their patterns and where each comes from,
 * and how many of the repository's files they match (a role matching nothing is reported,
 * never guessed).
 */
export function roleFiles(ctx: Ctx): { name: string; summary: string; patterns: string[]; sources: string[]; matching: number; notes: string[]; rules: string[] }[] {
  if (ctx.level !== "project" || !ctx.plan.roles.length) return [];
  const files = listRepoFiles(ctx.base);
  return ctx.plan.roles.map((r) => {
    const inc = r.patterns.filter((p) => !p.exclude).map((p) => globToRegExp(p.glob));
    const exc = r.patterns.filter((p) => p.exclude).map((p) => globToRegExp(p.glob));
    const matching = files.filter((f) => inc.some((re) => re.test(f)) && !exc.some((re) => re.test(f))).length;
    return { name: r.name, summary: roleSummary(r), patterns: roleGlobs(r), sources: [...new Set(r.patterns.map((p) => p.source))], matching, notes: r.notes, rules: r.rules };
  });
}
