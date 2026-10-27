import { existsSync } from "node:fs";
import { join } from "node:path";
import { readToml } from "./core/fs.js";
import { lockPath, pluginRoot, settingsFile as settingsFileOf } from "./core/paths.js";
import { AUDITED_TOOLS, type AuditedTool, checkTools, commandsOf, PROJECT_TOOLS } from "./core/tools.js";
import { depMatches, detectAnywhere, detectPack, lineage, loadLocalRules, loadProjectConfig, readProjectFacts, type DepFacts, type ProjectFacts } from "./packs.js";
import {
  ENGINE_ROLES,
  astGrepRulePath,
  astGrepTestPath,
  fill,
  helperFiles,
  renderAstGrepRule,
  renderAstGrepTest,
  renderChecks,
  renderCheckScript,
  renderGuardRules,
  renderHooks,
  renderLib,
  renderPlugin,
  renderSgConfig,
  renderTestRules,
  template,
  typesLanguage,
  type TypesLanguage,
  type ActiveCheck,
  type ActiveGrep,
  type ActiveGuard,
} from "./render.js";
import { applyRoles, defaultRoles, resolveRoles, rolesOfRule, type RolePattern, type Roles } from "./roles.js";
import { listRepoFiles, TY_VERSION } from "./util.js";
import type { AstGrepTarget, CheckTarget, DesiredEntry, Level, Pack, ProjectConfig, Rule, ToolName, ToolTarget } from "./types.js";

export type { AuditedTool, DesiredEntry };

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

/** A file role an active rule names (§15.4), as forge status shows it. */
export interface RoleStatus {
  name: string;
  patterns: RolePattern[];
  notes: string[];
  /** The active rules naming it. */
  rules: string[];
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
  /** Rules of active packs left out by their version gate (`when`), with the dependencies they need. */
  gated: { id: string; needs: string }[];
  /** Tool diagnostics an active pack switches off (pack.yaml tool_off), with the rule targets that asked for them. */
  switchedOff: { pack: string; tool: string; expects: string; reason: string; rules: string[] }[];
  deprecated: { id: string; replaced_by: string }[];
  warnings: string[];
  tools: Record<AuditedTool, boolean>;
  /** Where each audited tool's audits run: "" (the root) and/or every workspace package that configures it. */
  toolDirs: Record<AuditedTool, string[]>;
  /** Project level: the checks fragment the hook scripts share (forge status runs its forge_missing_tools). */
  checksFragment?: string;
  /** The types engines the active rules use (§15.2). */
  types: TypesLanguage[];
  /** The file roles the active rules name, with their effective patterns (§15.4). */
  roles: RoleStatus[];
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
  /** The file roles' effective patterns (§15.4); default: Forge's defaults. */
  roles?: Roles;
}

/** Reads the project inputs (forge.yaml, .forge/rules, dependency facts) for planProject. */
export function readProjectInputs(repo: string): Pick<PlanOptions, "config" | "localRules" | "facts" | "roles"> {
  const config = loadProjectConfig(repo);
  const files = listRepoFiles(repo);
  return {
    config,
    localRules: loadLocalRules(repo),
    facts: readProjectFacts(repo, () => files),
    roles: resolveRoles({ repo, files }, config?.files),
  };
}

// ---------------------------------------------------------------- tool presence (static)

/** ESLint's flat config files (ESLint 9 and later read nothing else). */
export const ESLINT_CONFIGS = ["eslint.config.js", "eslint.config.mjs", "eslint.config.cjs", "eslint.config.ts", "eslint.config.mts", "eslint.config.cts"];

/** Whether each audited tool has a config file in dir, and whether its package depends on the tool. */
function toolsIn(dir: string, deps: DepFacts): Record<AuditedTool, { config: boolean; dep: boolean }> {
  const has = (f: string) => existsSync(join(dir, f));
  const py = readToml(join(dir, "pyproject.toml")) ?? {};
  return {
    biome: { config: has("biome.json") || has("biome.jsonc"), dep: deps.npm.has("@biomejs/biome") },
    ruff: { config: has("ruff.toml") || has(".ruff.toml") || py.tool?.ruff !== undefined, dep: deps.pypi.has("ruff") },
    // Forge's own ty (§15.2): wherever there is a Python project; its config is optional
    ty: { config: has("pyproject.toml") || has("ty.toml"), dep: false },
    tsc: { config: has("tsconfig.json"), dep: deps.npm.has("typescript") },
    // ESLint 9 and later: the flat config only
    eslint: { config: ESLINT_CONFIGS.some(has), dep: deps.npm.has("eslint") },
  };
}

/**
 * Where each audited tool is configured or depended on: "" for the root, else
 * workspace package directories. A root that only depends on a tool does not
 * count when a workspace package has the tool's config file (the config is
 * there, and so is where the tool must run).
 */
export function toolPlaces(repo: string, facts: ProjectFacts): Record<AuditedTool, string[]> {
  const root = toolsIn(repo, facts.root);
  const ws = facts.workspaces.map((w) => ({ dir: w.dir, tools: toolsIn(join(repo, w.dir), w) }));
  const out = {} as Record<AuditedTool, string[]>;
  for (const t of AUDITED_TOOLS) {
    const atRoot = root[t].config || (root[t].dep && !ws.some((w) => w.tools[t].config));
    out[t] = [...(atRoot ? [""] : []), ...ws.filter((w) => w.tools[t].config || w.tools[t].dep).map((w) => w.dir)];
  }
  return out;
}

/** Engine helpers that work the same from any directory: forge_exec_tool runs the project's tool (a name with "-", e.g. vue-tsc); forge_state_file names a per-package file under the root's .forge/state (tool caches). */
const CWD_AGNOSTIC_HELPERS = new Set(["forge_exec_tool", "forge_state_file"]);

/**
 * Checks that may run inside workspace packages: those that run a project tool
 * (which reads its config from the working directory) and no engine helper other
 * than the cwd-agnostic ones (the others work on repository paths: ratchet,
 * migrations, Docker).
 */
function perPackage(t: CheckTarget): boolean {
  const commands = commandsOf(t).join("\n");
  const helpers = commands.match(/\bforge_[a-z_]+/g) ?? [];
  return helpers.every((h) => CWD_AGNOSTIC_HELPERS.has(h)) && checkTools(t).some((x) => (PROJECT_TOOLS as readonly string[]).includes(x));
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
      if ((p.parent === undefined || why.has(p.parent)) && opts.facts && detectAnywhere(p, opts.facts)) changed = activate(p.id, "detected") || changed;
    }
    for (const id of [...why.keys()]) for (const req of opts.packs.get(id)!.requires) for (const a of lineage(opts.packs, req)) changed = activate(a, `required by ${id}`) || changed;
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
  const places = level === "project" && opts.facts ? toolPlaces(base, opts.facts) : undefined;
  const tools = Object.fromEntries(AUDITED_TOOLS.map((t) => [t, (places?.[t].length ?? 0) > 0])) as Record<AuditedTool, boolean>;
  const toolDirs = Object.fromEntries(AUDITED_TOOLS.map((t) => [t, places?.[t].length ? places[t] : [""]])) as Record<AuditedTool, string[]>;
  const workspaces = level === "project" ? (opts.facts?.workspaces ?? []) : [];

  /**
   * Where a check runs: at the root (dirs undefined) when its pack is there and the
   * root configures its audited tools; otherwise in each workspace package that has
   * its pack (or any package, for a pack active at the root) and configures them.
   */
  /**
   * The workspace packages where a pack was detected, or undefined when it applies to
   * the whole repository (detected at the root, added in forge.yaml, required, a
   * parent of an added pack, or a local rule's pack outside the tree).
   */
  const packDirs = (id: string): string[] | undefined => {
    const pack = opts.packs.get(id);
    if (!pack || !opts.facts || !workspaces.length || packs.find((p) => p.id === id)?.why !== "detected" || detectPack(pack, opts.facts.root)) return undefined;
    return workspaces.filter((w) => detectPack(pack, w)).map((w) => w.dir);
  };

  /**
   * Where a rule applies: its pack's places (packDirs), narrowed to those where its
   * version gate holds (every dependency of `when` declared in its range). A pack that
   * applies to the whole repository is judged at the root, else (added in forge.yaml,
   * required) in any workspace package. null: the gate holds nowhere.
   */
  const gateHolds = (rule: Rule, facts: DepFacts) => (rule.when?.deps ?? []).every((d) => depMatches(facts, d));
  const ruleDirs = (rule: Rule): string[] | undefined => {
    const where = packDirs(rule.pack);
    if (!rule.when || !opts.facts) return where;
    if (where) return workspaces.filter((w) => where.includes(w.dir) && gateHolds(rule, w)).map((w) => w.dir);
    return gateHolds(rule, opts.facts.root) ? undefined : workspaces.filter((w) => gateHolds(rule, w)).map((w) => w.dir);
  };
  /**
   * The packages an ast-grep target with a where needs (type stubs) that are not declared
   * where the rule applies: at the root (or in any package) for a repository-wide rule,
   * in each of its packages otherwise.
   */
  const missingNeeds = (rule: Rule, t: AstGrepTarget): string[] => {
    if (!t.needs || !opts.facts) return t.needs && !opts.facts ? t.needs.map((n) => n.name) : [];
    const dirs = ruleDirs(rule);
    const places: DepFacts[] = dirs ? workspaces.filter((w) => dirs.includes(w.dir)) : [opts.facts.root, ...workspaces];
    const has = (f: DepFacts, n: { ecosystem: "pypi" | "npm"; name: string }) => depMatches(f, n);
    return t.needs.filter((n) => (dirs ? !places.every((f) => has(f, n)) : !places.some((f) => has(f, n)))).map((n) => n.name);
  };
  const gatedOut = (rule: Rule): boolean => {
    if (!rule.when || !opts.facts) return false;
    return ruleDirs(rule)?.length === 0;
  };

  const checkPlace = (rule: Rule, t: CheckTarget): { dirs?: string[]; absent: string[] } => {
    const audited = checkTools(t).filter((x): x is AuditedTool => (AUDITED_TOOLS as readonly string[]).includes(x));
    const absent = audited.filter((x) => !tools[x]);
    if (absent.length || !places || !opts.facts || !workspaces.length || !perPackage(t)) return { absent };
    const where = ruleDirs(rule);
    if (!where && audited.every((x) => places[x].includes(""))) return { absent: [] };
    const dirs = workspaces.filter((w) => (!where || where.includes(w.dir)) && audited.every((x) => places[x].includes(w.dir))).map((w) => w.dir);
    return dirs.length ? { dirs, absent: [] } : { absent: audited };
  };

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

  const gated: Plan["gated"] = [];
  for (const [id, r] of byId)
    if (level === "project" && gatedOut(r)) {
      byId.delete(id);
      gated.push({ id, needs: r.when!.deps.map((d) => `${d.name} ${d.version}`).join(", ") });
    }

  // file roles (§15.4): each rule as apply renders it, its roles replaced by their effective patterns
  const roles = opts.roles ?? defaultRoles();
  const roleUse = new Map<string, string[]>();
  const emptyRoles = new Map<string, string[]>();
  const rulesSorted = [...byId.values()]
    .sort((a, b) => a.id.localeCompare(b.id))
    .map((r) => {
      for (const name of rolesOfRule(r)) roleUse.set(name, [...(roleUse.get(name) ?? []), r.id]);
      const { rule, empty } = applyRoles(r, roles);
      if (empty.length) emptyRoles.set(r.id, empty);
      return rule;
    });
  const active: RuleStatus[] = [];
  const checks: ActiveCheck[] = [];
  const greps: ActiveGrep[] = [];
  const guards: ActiveGuard[] = [];
  const settings: DesiredEntry[] = [];
  const settingsFile = settingsFileOf(level, opts.local);
  const astgrep: { rule: Rule; target: AstGrepTarget; dirs?: string[] }[] = [];
  const present = (t: ToolName) => tools[t];

  // tool diagnostics an active pack switches off: no rule's tool target asks for them (pack.yaml tool_off)
  const switchedOff: Plan["switchedOff"] = packs.flatMap((pk) => (opts.packs.get(pk.id)!.tool_off ?? []).map((o) => ({ pack: pk.id, ...o, rules: [] as string[] })));
  const offBy = (t: ToolTarget) => switchedOff.find((o) => o.tool === t.tool && o.expects === t.expects);

  for (const rule of rulesSorted) {
    const st: RuleStatus = { rule, enforced: [], notEnforced: [], audits: [] };
    const empty = emptyRoles.get(rule.id);
    if (empty) {
      // never guessed: a rule whose files are a role with no pattern is reported, not run on other files
      st.notEnforced.push(`not enforced here: file role ${empty.map((e) => `@${e}`).join(", ")} has no files (set them in forge.yaml: files: {${empty[0]}: {set: [...]}})`);
      active.push(st);
      continue;
    }
    const toolTargets = rule.enforce.filter((t): t is ToolTarget => t.target === "tool");
    const astTargets = rule.enforce.filter((t): t is AstGrepTarget => t.target === "ast-grep");
    if (astTargets.length > 1) throw new Error(`${rule.file}: at most one ast-grep target per rule`);
    for (const t of toolTargets) {
      const off = offBy(t);
      if (off) {
        off.rules.push(rule.id);
        st.notEnforced.push(`not enforced here: ${t.tool} ${t.expects} switched off by ${off.pack} (${off.reason})`);
      } else if (present(t.tool)) {
        st.audits.push(t);
        st.enforced.push(`${t.tool} (${t.expects}), audited`);
      } else st.notEnforced.push(`not enforced here: ${t.tool} not configured`);
    }
    const ast = astTargets[0];
    // a switched-off diagnostic is not asked for in any form: no ast-grep stand-in either
    if (ast && !toolTargets.some((t) => offBy(t)) && (toolTargets.length === 0 || !toolTargets.some((t) => present(t.tool)))) {
      if (level === "user") warnings.push(`${rule.id}: ast-grep targets are project-level only`);
      else if (missingNeeds(rule, ast).length) {
        st.notEnforced.push(`not enforced here: the types engine needs ${missingNeeds(rule, ast).join(", ")} installed to see the types this rule checks (add it as a development dependency)`);
      } else {
        const dirs = ruleDirs(rule);
        st.astgrep = ast;
        astgrep.push({ rule, target: ast, ...(dirs ? { dirs } : {}) });
        const lang = typesLanguage(rule, ast);
        const engine = lang === "python" ? `ty ${TY_VERSION}` : "the project's TypeScript";
        st.enforced.push(`ast-grep ${ast.mode} (.forge/ast-grep/${astGrepRulePath(rule.id)})${lang ? ` with types (${engine})` : ""}${dirs ? ` (in ${dirs.join(", ")})` : ""}`);
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
        const dirs = ruleDirs(rule);
        greps.push({ rule, target: t, ...(dirs ? { dirs } : {}) });
        st.enforced.push(`grep ${t.mode} (.forge/check.sh)${dirs ? ` (in ${dirs.join(", ")})` : ""}`);
      } else if (t.target === "check") {
        if (level === "user") {
          warnings.push(`${rule.id}: check targets are project-level only`);
          continue;
        }
        const { dirs, absent } = checkPlace(rule, t);
        if (absent.length) st.notEnforced.push(`not enforced here: ${absent.join(", ")} not configured`);
        else {
          checks.push({ rule, target: t, ...(dirs ? { dirs } : {}) });
          st.enforced.push(`check ${t.stage}: ${t.run}${dirs ? ` (in ${dirs.join(", ")})` : ""}`);
        }
      }
    }
    active.push(st);
  }

  // ---- files
  const files = new Map<string, string>();
  let checksFragment: string | undefined;
  let typesUsed: TypesLanguage[] = [];
  const lib = renderLib();
  const guardRules = renderGuardRules(guards, base);
  if (level === "user") {
    const root = pluginRoot("user");
    files.set(`${root}/.claude-plugin/plugin.json`, renderPlugin("user"));
    files.set(`${root}/hooks/hooks.json`, renderHooks("user"));
    files.set(`${root}/scripts/lib.sh`, lib);
    files.set(`${root}/scripts/guard.sh`, template("guard.sh"));
    files.set(`${root}/scripts/guard.rules`, guardRules);
    files.set(`${root}/scripts/notify.sh`, template("notify.sh"));
    files.set(`${root}/scripts/review.sh`, template("review.sh"));
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
    // the types engines the active ast-grep rules use (§15.2)
    const types = [...new Set(astgrep.map((a) => typesLanguage(a.rule, a.target)).filter((l): l is TypesLanguage => !!l))].sort();
    // forge_role knows the roles the active rules and the engine's helpers name
    const rendered: Roles = new Map([...roles].filter(([name]) => roleUse.has(name) || ENGINE_ROLES.includes(name)));
    const fragment = renderChecks(checks, greps, types, rendered);
    checksFragment = fragment;
    typesUsed = types;
    const root = pluginRoot("project");
    files.set(`${root}/.claude-plugin/plugin.json`, renderPlugin("project"));
    files.set(`${root}/hooks/hooks.json`, renderHooks("project", { types: types.length > 0 }));
    files.set(`${root}/scripts/lib.sh`, lib);
    files.set(`${root}/scripts/guard.sh`, template("guard.sh"));
    files.set(`${root}/scripts/guard.rules`, guardRules);
    files.set(`${root}/scripts/review.sh`, template("review.sh"));
    files.set(`${root}/scripts/session-start.sh`, fill(template("session-start.sh"), { CHECKS: fragment, PACKS: packs.map((p) => p.id).join(", ") || "none" }));
    files.set(`${root}/scripts/bash-snapshot.sh`, template("bash-snapshot.sh"));
    files.set(`${root}/scripts/post-edit.sh`, fill(template("post-edit.sh"), { CHECKS: fragment, FORBID: [ids("forbid"), ids("advise")].filter(Boolean).join("|") }));
    files.set(`${root}/scripts/typecheck.sh`, fill(template("typecheck.sh"), { CHECKS: fragment }));
    files.set(`${root}/scripts/stop.sh`, template("stop.sh"));
    files.set(`${root}/scripts/test-edit.sh`, fill(template("test-edit.sh"), { CHECKS: fragment, TEST_RULES: renderTestRules(checks) }));
    files.set(`${root}/scripts/turn-start.sh`, template("turn-start.sh"));
    if (types.length) files.set(`${root}/scripts/session-end.sh`, template("session-end.sh"));
    for (const p of packs) {
      const ctx = opts.packs.get(p.id)!.context;
      if (ctx !== undefined) files.set(`.claude/rules/${contextFileName(p.id)}`, CONTEXT_INTRO + ctx);
    }
    files.set(".forge/check.sh", renderCheckScript(checks, { forbid: ids("forbid"), noIncrease: ids("no-increase"), noDecrease: ids("no-decrease") }, greps, types, rendered));
    files.set(".forge/.gitignore", "state/\n");
    for (const [path, text] of helperFiles(checks, types)) files.set(path, text);
    files.set(".forge/ast-grep/sgconfig.yml", renderSgConfig());
    for (const { rule, target, dirs } of astgrep) {
      files.set(`.forge/ast-grep/${astGrepRulePath(rule.id)}`, renderAstGrepRule(rule, target, dirs));
      if (rule.examples?.match?.length || rule.examples?.no_match?.length) files.set(`.forge/ast-grep/${astGrepTestPath(rule.id)}`, renderAstGrepTest(rule));
    }
    settings.unshift(
      ...["Edit(.forge/**)", "Edit(forge.yaml)", "Edit(.claude/skills/forge-project/**)", "Edit(.claude/rules/forge-*.md)"].map((value) => ({
        file: settingsFile,
        path: "permissions.ask",
        value,
        append: true,
      })),
    );
  }

  const rules: Record<string, string> = {};
  for (const r of rulesSorted) rules[r.id] = r.hash;
  return {
    level,
    base,
    files: new Map([...files.entries()].sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0))),
    lockPath: lockPath(level),
    settingsFile,
    settings: dedupe(settings),
    rules,
    packs,
    removedPacks: removed,
    active,
    disabled,
    gated,
    switchedOff,
    deprecated,
    warnings,
    tools,
    toolDirs,
    checksFragment,
    types: typesUsed,
    roles:
      level === "project"
        ? [...roleUse.entries()].sort(([a], [b]) => (a < b ? -1 : 1)).map(([name, ids]) => ({ name, patterns: roles.get(name)!.patterns, notes: roles.get(name)!.notes, rules: ids }))
        : [],
  };
}

/**
 * First line of every generated context file: the facts guide defaults, and an
 * explicit instruction wins (a rule followed against a clear request failed a task).
 */
export const CONTEXT_INTRO =
  "These are this project's defaults. When the task or the user explicitly asks for something different, do what they ask and point out the conflict; never override an explicit instruction with these defaults.\n\n";

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
