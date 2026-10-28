/**
 * The plan (§3.1): what forge apply writes and appends at one level, and what forge status
 * says about each rule. The phases, in order: the active packs, where things apply, the
 * rules that apply, what each of their targets enforces, and the files to write.
 */
import { lockPath, settingsFile as settingsFileOf } from "../core/paths.js";
import { kindOf, targetsByPhase } from "../targets/index.js";
import type { PlanContext } from "../targets/kind.js";
import type { DesiredEntry, Rule, RuleStatus } from "../types.js";
import { activePacks } from "./active-packs.js";
import { projectFiles, userFiles } from "./files.js";
import { checkPlace, missingNeeds, ruleDirs, site, toolDirs } from "./placement.js";
import { selectRules } from "./rules.js";
import type { Plan, PlanOptions } from "./types.js";
import { DEFAULT_ENGINE_SETTINGS, type EngineSettings } from "../render/index.js";

export type { AuditedTool } from "../core/tools.js";
export type { DesiredEntry, RuleStatus } from "../types.js";
export { CONTEXT_INTRO, contextFileName } from "./files.js";
export { readProjectInputs } from "./inputs.js";
export { ESLINT_CONFIGS, toolPlaces } from "./placement.js";
export type { PackStatus, Plan, PlanOptions, RoleStatus } from "./types.js";

export function plan(opts: PlanOptions): Plan {
  const { level, base } = opts;
  const warnings: string[] = [];
  const { packs, removed } = activePacks(opts, warnings);
  const where = site(level, base, opts.packs, opts.facts, packs);
  const selected = selectRules(opts, where, warnings);

  // what each target of each rule enforces here
  const settingsFile = settingsFileOf(level, opts.local);
  // tool diagnostics an active pack switches off: no rule's tool target asks for them (pack.yaml tool_off)
  const switchedOff: Plan["switchedOff"] = packs.flatMap((pk) => (opts.packs.get(pk.id)!.tool_off ?? []).map((o) => ({ pack: pk.id, ...o, rules: [] as string[] })));
  const ctx: PlanContext = {
    level,
    settingsFile,
    warnings,
    out: { settings: [], guards: [], greps: [], checks: [], astgrep: [] },
    present: (t) => where.tools[t],
    switchedOff: (t) => switchedOff.find((o) => o.tool === t.tool && o.expects === t.expects),
    ruleDirs: (rule) => ruleDirs(where, rule),
    missingNeeds: (rule, t) => missingNeeds(where, rule, t),
    checkPlace: (rule, t) => checkPlace(where, rule, t),
  };
  const active = selected.rules.map((rule) => enforceRule(ctx, rule, selected.emptyRoles.get(rule.id)));

  const out =
    level === "user"
      ? { files: userFiles(ctx, base), fragment: undefined, types: [] }
      : projectFiles(ctx, base, { active: packs, all: opts.packs }, { all: selected.roles, used: selected.roleUse }, engineSettings(opts));
  return {
    level,
    base,
    files: new Map([...out.files.entries()].sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0))),
    lockPath: lockPath(level),
    settingsFile,
    settings: dedupe(ctx.out.settings),
    rules: Object.fromEntries(selected.rules.map((r) => [r.id, r.hash])),
    packs,
    removedPacks: removed,
    active,
    disabled: selected.disabled,
    gated: selected.gated,
    switchedOff,
    deprecated: selected.deprecated,
    warnings,
    tools: where.tools,
    toolDirs: toolDirs(where),
    checksFragment: out.fragment,
    types: out.types,
    engineSettings: engineSettings(opts),
    roles:
      level === "project"
        ? [...selected.roleUse.entries()]
            .sort(([a], [b]) => (a < b ? -1 : 1))
            .map(([name, ids]) => ({ name, patterns: selected.roles.get(name)!.patterns, notes: selected.roles.get(name)!.notes, rules: ids }))
        : [],
  };
}

/** What a rule's targets enforce here, in the plan's phase order (tool, the ast-grep stand-in, then the rest). */
function enforceRule(ctx: PlanContext, rule: Rule, emptyRoles: string[] | undefined): RuleStatus {
  const st: RuleStatus = { rule, enforced: [], notEnforced: [], audits: [] };
  if (emptyRoles) {
    // never guessed: a rule whose files are a role with no pattern is reported, not run on other files
    st.notEnforced.push(`not enforced here: file role ${emptyRoles.map((e) => `@${e}`).join(", ")} has no files (set them in forge.yaml: files: {${emptyRoles[0]}: {set: [...]}})`);
    return st;
  }
  if (rule.enforce.filter((t) => t.target === "ast-grep").length > 1) throw new Error(`${rule.file}: at most one ast-grep target per rule`);
  for (const t of targetsByPhase(rule)) kindOf(t).plan(ctx, rule, st, t);
  return st;
}

/** The forge.yaml settings the generated engine is rendered with. */
export function engineSettings(opts: Pick<PlanOptions, "config">): EngineSettings {
  return { untypedPackages: opts.config?.python?.["untyped-packages"] ?? DEFAULT_ENGINE_SETTINGS.untypedPackages };
}

/** Settings entries once each, in the order they were first wanted. */
function dedupe(entries: DesiredEntry[]): DesiredEntry[] {
  const seen = new Set<string>();
  return entries.filter((e) => {
    const k = `${e.file}\0${e.path}\0${JSON.stringify(e.value)}`;
    if (seen.has(k)) return false;
    seen.add(k);
    return true;
  });
}
