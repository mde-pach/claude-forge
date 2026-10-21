import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { parse as parseToml } from "smol-toml";
import { detectPack, lineage, loadLocalRules, loadProjectConfig, readProjectFacts, type ProjectFacts } from "./packs.js";
import {
  AUDITED_TOOLS,
  astGrepRulePath,
  astGrepTestPath,
  checkTools,
  fill,
  renderAstGrepRule,
  renderAstGrepTest,
  renderChecks,
  renderCheckScript,
  renderGuardRules,
  renderHooks,
  renderLib,
  renderPlugin,
  renderSgConfig,
  template,
  type ActiveCheck,
  type ActiveGrep,
  type ActiveGuard,
} from "./render.js";
import { listRepoFiles } from "./util.js";
import type { AstGrepTarget, Level, Pack, ProjectConfig, Rule, SettingsEntry, ToolName, ToolTarget } from "./types.js";

export type AuditedTool = (typeof AUDITED_TOOLS)[number];

export interface DesiredEntry extends SettingsEntry {
  /** Append `value` to an array at `path` (else: set the scalar if absent). */
  append: boolean;
}

export interface PackStatus {
  id: string;
  why: string;
}

export interface RuleStatus {
  rule: Rule;
  /** Where the rule is enforced in this project. */
  enforced: string[];
  /** "not enforced here: …" notes. */
  notEnforced: string[];
  /** Tool targets whose tool is present: audited (§11). */
  audits: ToolTarget[];
  astgrep?: AstGrepTarget;
}

export interface Plan {
  level: Level;
  base: string;
  /** Owned files, relative to base. */
  files: Map<string, string>;
  lockPath: string;
  settingsFile: string;
  settings: DesiredEntry[];
  rules: Record<string, string>;
  packs: PackStatus[];
  removedPacks: PackStatus[];
  active: RuleStatus[];
  disabled: { id: string; reason: string }[];
  deprecated: { id: string; replaced_by: string }[];
  warnings: string[];
  tools: Record<AuditedTool, boolean>;
}

export interface PlanOptions {
  level: Level;
  /** Repository root (project) or home directory (user). */
  base: string;
  packs: Map<string, Pack>;
  /** Project only: permission entries go to .claude/settings.local.json. */
  local?: boolean;
  config?: ProjectConfig;
  localRules?: Rule[];
  facts?: ProjectFacts;
}

/** Reads the project inputs (forge.yaml, .forge/rules, dependency facts) for planProject. */
export function readProjectInputs(repo: string): Pick<PlanOptions, "config" | "localRules" | "facts"> {
  return {
    config: loadProjectConfig(repo),
    localRules: loadLocalRules(repo),
    facts: readProjectFacts(repo, () => listRepoFiles(repo)),
  };
}

// ---------------------------------------------------------------- tool presence (static)

function readPyproject(repo: string): Record<string, any> {
  const f = join(repo, "pyproject.toml");
  if (!existsSync(f)) return {};
  try {
    return parseToml(readFileSync(f, "utf8")) as Record<string, any>;
  } catch {
    return {};
  }
}

/** Whether each audited tool is configured or depended on by the project. */
export function staticTools(repo: string, facts: ProjectFacts): Record<AuditedTool, boolean> {
  const has = (f: string) => existsSync(join(repo, f));
  const py = readPyproject(repo);
  return {
    biome: has("biome.json") || has("biome.jsonc") || facts.npm.has("@biomejs/biome"),
    ruff: has("ruff.toml") || has(".ruff.toml") || py.tool?.ruff !== undefined || facts.pypi.has("ruff"),
    basedpyright: facts.pypi.has("basedpyright") || py.tool?.basedpyright !== undefined || has("pyrightconfig.json"),
    tsc: has("tsconfig.json") || facts.npm.has("typescript"),
  };
}

// ---------------------------------------------------------------- active packs

function activePacks(opts: PlanOptions, warnings: string[]): { packs: PackStatus[]; removed: PackStatus[] } {
  const all = [...opts.packs.values()];
  if (opts.level === "user") return { packs: all.filter((p) => p.level === "user").map((p) => ({ id: p.id, why: "user level" })), removed: [] };
  const why = new Map<string, string>();
  const activate = (id: string, reason: string) => {
    if (why.has(id)) return false;
    why.set(id, reason);
    return true;
  };
  // forge.yaml add: the pack and its ancestors
  for (const id of opts.config?.packs?.add ?? []) {
    const p = opts.packs.get(id);
    if (!p) warnings.push(`forge.yaml: packs.add: unknown pack "${id}"`);
    else if (p.level !== "project") warnings.push(`forge.yaml: packs.add: "${id}" is a user-level pack (use forge apply --user)`);
    else {
      for (const a of lineage(opts.packs, id).slice(0, -1)) activate(a, `parent of ${id}`);
      activate(id, "forge.yaml");
    }
  }
  for (const id of opts.config?.packs?.remove ?? []) if (!opts.packs.has(id)) warnings.push(`forge.yaml: packs.remove: unknown pack "${id}"`);
  // detection down the tree (a child only under an active parent), and requires, to a fixpoint
  for (let changed = true; changed; ) {
    changed = false;
    for (const p of all) {
      if (p.level !== "project" || why.has(p.id)) continue;
      if ((p.parent === undefined || why.has(p.parent)) && opts.facts && detectPack(p, opts.facts)) changed = activate(p.id, "detected") || changed;
    }
    for (const id of [...why.keys()])
      for (const req of opts.packs.get(id)!.requires)
        for (const a of lineage(opts.packs, req)) changed = activate(a, `required by ${id}`) || changed;
  }
  // removal: the pack, its subtree, and every pack requiring a removed one
  const removed = new Map<string, string>();
  for (const id of opts.config?.packs?.remove ?? []) if (why.has(id)) removed.set(id, "removed in forge.yaml");
  for (let changed = true; changed; ) {
    changed = false;
    for (const id of why.keys()) {
      if (removed.has(id)) continue;
      const p = opts.packs.get(id)!;
      const req = p.requires.find((r) => removed.has(r));
      const reason = p.parent && removed.has(p.parent) ? `parent ${p.parent} is not active` : req ? `requires removed pack ${req}` : undefined;
      if (reason) {
        removed.set(id, reason);
        changed = true;
      }
    }
  }
  const order = (a: string, b: string) => (a < b ? -1 : a > b ? 1 : 0);
  const packs = [...why.entries()]
    .filter(([id]) => !removed.has(id))
    .sort(([a], [b]) => order(a, b))
    .map(([id, w]) => ({ id, why: w }));
  return { packs, removed: [...removed.entries()].sort(([a], [b]) => order(a, b)).map(([id, w]) => ({ id, why: w })) };
}

// ---------------------------------------------------------------- plan

export function plan(opts: PlanOptions): Plan {
  const { level, base } = opts;
  const warnings: string[] = [];
  const { packs, removed } = activePacks(opts, warnings);
  const tools =
    level === "project" && opts.facts
      ? staticTools(base, opts.facts)
      : ({ biome: false, ruff: false, basedpyright: false, tsc: false } as Record<AuditedTool, boolean>);

  // rules: pack rules of active packs, replaced by local rules with the same id
  const byId = new Map<string, Rule>();
  for (const p of packs) for (const r of opts.packs.get(p.id)!.rules) byId.set(r.id, r);
  if (level === "project")
    for (const r of opts.localRules ?? []) {
      if (byId.has(r.id) || !opts.packs.has(r.pack) || packs.some((p) => p.id === r.pack)) byId.set(r.id, r);
      else warnings.push(`${r.file}: pack "${r.pack}" is not active here; rule ${r.id} is ignored`);
    }

  const disabled: Plan["disabled"] = [];
  for (const d of level === "project" ? (opts.config?.disable ?? []) : []) {
    if (!byId.has(d.id)) warnings.push(`forge.yaml: disable: rule "${d.id}" is not active here`);
    else {
      byId.delete(d.id);
      disabled.push(d);
    }
  }
  const deprecated: Plan["deprecated"] = [];
  for (const [id, r] of byId)
    if (r.deprecated) {
      byId.delete(id);
      deprecated.push({ id, replaced_by: r.deprecated.replaced_by });
    }

  const rulesSorted = [...byId.values()].sort((a, b) => a.id.localeCompare(b.id));
  const active: RuleStatus[] = [];
  const checks: ActiveCheck[] = [];
  const greps: ActiveGrep[] = [];
  const guards: ActiveGuard[] = [];
  const settings: DesiredEntry[] = [];
  const settingsFile = level === "user" ? ".claude/settings.json" : opts.local ? ".claude/settings.local.json" : ".claude/settings.json";
  const astgrep: { rule: Rule; target: AstGrepTarget }[] = [];
  const present = (t: ToolName) => tools[t];

  for (const rule of rulesSorted) {
    const st: RuleStatus = { rule, enforced: [], notEnforced: [], audits: [] };
    const toolTargets = rule.enforce.filter((t): t is ToolTarget => t.target === "tool");
    const astTargets = rule.enforce.filter((t): t is AstGrepTarget => t.target === "ast-grep");
    if (astTargets.length > 1) throw new Error(`${rule.file}: at most one ast-grep target per rule`);
    for (const t of toolTargets) {
      if (present(t.tool)) {
        st.audits.push(t);
        st.enforced.push(`${t.tool} (${t.expects}), audited`);
      } else st.notEnforced.push(`not enforced here: ${t.tool} not configured`);
    }
    const ast = astTargets[0];
    if (ast && (toolTargets.length === 0 || !toolTargets.some((t) => present(t.tool)))) {
      if (level === "user") warnings.push(`${rule.id}: ast-grep targets are project-level only`);
      else {
        st.astgrep = ast;
        astgrep.push({ rule, target: ast });
        st.enforced.push(`ast-grep ${ast.mode} (.forge/ast-grep/${astGrepRulePath(rule.id)})`);
        st.notEnforced = [];
      }
    }
    for (const t of rule.enforce) {
      if (t.target === "permission") {
        settings.push({ file: settingsFile, path: `permissions.${t.decision}`, value: t.rule, append: true });
        st.enforced.push(`permission ${t.decision} ${t.rule} (${settingsFile})`);
      } else if (t.target === "guard") {
        guards.push({ rule, target: t });
        if (!st.enforced.some((e) => e.startsWith("guard"))) st.enforced.push(`guard ${t.decision} (PreToolUse Bash)`);
      } else if (t.target === "sandbox") {
        if (level !== "user") {
          warnings.push(`${rule.id}: sandbox targets are user-level only`);
          continue;
        }
        const path = `sandbox.${t.path}`;
        if (Array.isArray(t.value)) for (const v of t.value) settings.push({ file: settingsFile, path, value: v, append: true });
        else settings.push({ file: settingsFile, path, value: t.value, append: false });
        st.enforced.push(`sandbox ${t.path} (${settingsFile})`);
      } else if (t.target === "grep") {
        if (level === "user") {
          warnings.push(`${rule.id}: grep targets are project-level only`);
          continue;
        }
        greps.push({ rule, target: t });
        st.enforced.push(`grep ${t.mode} (.forge/check.sh)`);
      } else if (t.target === "check") {
        if (level === "user") {
          warnings.push(`${rule.id}: check targets are project-level only`);
          continue;
        }
        const absent = checkTools(t).filter((x): x is AuditedTool => (AUDITED_TOOLS as readonly string[]).includes(x) && !tools[x as AuditedTool]);
        if (absent.length) st.notEnforced.push(`not enforced here: ${absent.join(", ")} not configured`);
        else {
          checks.push({ rule, target: t });
          st.enforced.push(`check ${t.stage}: ${t.run}`);
        }
      }
    }
    active.push(st);
  }

  // ---- files
  const files = new Map<string, string>();
  const lib = renderLib();
  const guardRules = renderGuardRules(guards, base);
  if (level === "user") {
    const root = ".claude/skills/forge-user";
    files.set(`${root}/.claude-plugin/plugin.json`, renderPlugin("user"));
    files.set(`${root}/hooks/hooks.json`, renderHooks("user"));
    files.set(`${root}/scripts/lib.sh`, lib);
    files.set(`${root}/scripts/guard.sh`, template("guard.sh"));
    files.set(`${root}/scripts/guard.rules`, guardRules);
    files.set(`${root}/scripts/notify.sh`, template("notify.sh"));
    settings.push(
      { file: settingsFile, path: "permissions.ask", value: "Edit(~/.claude/skills/forge-user/**)", append: true },
      { file: settingsFile, path: "permissions.ask", value: "Edit(~/.claude/forge.lock.json)", append: true },
    );
  } else {
    const ids = (mode: AstGrepTarget["mode"]) =>
      astgrep
        .filter((a) => a.target.mode === mode)
        .map((a) => a.rule.id)
        .join("|");
    const fragment = renderChecks(checks, greps);
    const root = ".claude/skills/forge-project";
    files.set(`${root}/.claude-plugin/plugin.json`, renderPlugin("project"));
    files.set(`${root}/hooks/hooks.json`, renderHooks("project"));
    files.set(`${root}/scripts/lib.sh`, lib);
    files.set(`${root}/scripts/guard.sh`, template("guard.sh"));
    files.set(`${root}/scripts/guard.rules`, guardRules);
    files.set(`${root}/scripts/session-start.sh`, fill(template("session-start.sh"), { CHECKS: fragment, PACKS: packs.map((p) => p.id).join(", ") || "none" }));
    files.set(`${root}/scripts/post-edit.sh`, fill(template("post-edit.sh"), { CHECKS: fragment, FORBID: ids("forbid") }));
    files.set(`${root}/scripts/typecheck.sh`, fill(template("typecheck.sh"), { CHECKS: fragment }));
    files.set(`${root}/scripts/stop.sh`, template("stop.sh"));
    files.set(`${root}/scripts/turn-start.sh`, template("turn-start.sh"));
    for (const p of packs) {
      const ctx = opts.packs.get(p.id)!.context;
      if (ctx !== undefined) files.set(`.claude/rules/${contextFileName(p.id)}`, ctx);
    }
    files.set(".forge/check.sh", renderCheckScript(checks, { forbid: ids("forbid"), noIncrease: ids("no-increase"), noDecrease: ids("no-decrease") }, greps));
    files.set(".forge/.gitignore", "state/\n");
    files.set(".forge/ast-grep/sgconfig.yml", renderSgConfig());
    for (const { rule, target } of astgrep) {
      files.set(`.forge/ast-grep/${astGrepRulePath(rule.id)}`, renderAstGrepRule(rule, target));
      if (rule.examples?.match?.length || rule.examples?.no_match?.length)
        files.set(`.forge/ast-grep/${astGrepTestPath(rule.id)}`, renderAstGrepTest(rule));
    }
    settings.unshift(
      ...["Edit(.forge/**)", "Edit(forge.yaml)", "Edit(.claude/skills/forge-project/**)", "Edit(.claude/rules/forge-*.md)"].map(
        (value) => ({ file: settingsFile, path: "permissions.ask", value, append: true }),
      ),
    );
  }

  const rules: Record<string, string> = {};
  for (const r of rulesSorted) rules[r.id] = r.hash;
  return {
    level,
    base,
    files: new Map([...files.entries()].sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0))),
    lockPath: level === "user" ? ".claude/forge.lock.json" : ".forge/forge.lock.json",
    settingsFile,
    settings: dedupe(settings),
    rules,
    packs,
    removedPacks: removed,
    active,
    disabled,
    deprecated,
    warnings,
    tools,
  };
}

/** `.claude/rules/` file name for a pack's context: forge-<id with "/" as "-">.md. */
export function contextFileName(packId: string): string {
  return `forge-${packId.replaceAll("/", "-")}.md`;
}

function dedupe(entries: DesiredEntry[]): DesiredEntry[] {
  const seen = new Set<string>();
  return entries.filter((e) => {
    const k = `${e.file}\0${e.path}\0${JSON.stringify(e.value)}`;
    if (seen.has(k)) return false;
    seen.add(k);
    return true;
  });
}
