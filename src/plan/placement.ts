/**
 * Where things apply in a repository (§5.1): which audited tools are configured where, in
 * which workspace packages a pack was detected, where a rule's version gate holds, and
 * where a check runs.
 */
import { existsSync } from "node:fs";
import { join } from "node:path";
import { readToml } from "../core/fs.js";
import { AUDITED_TOOLS, type AuditedTool, checkTools, commandsOf, PROJECT_TOOLS } from "../core/tools.js";
import { type DepFacts, depMatches, detectPack, type ProjectFacts, type WorkspaceFacts } from "../packs.js";
import type { AstGrepTarget, CheckTarget, Level, Pack, Rule } from "../types.js";
import type { PackStatus } from "./types.js";

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

/** Engine helpers that work the same from any directory: forge_do and forge_exec_tool run the project's tool (§16: as its adapter says; a name with "-", e.g. vue-tsc); forge_state_file names a per-package file under the root's .forge/state (tool caches). */
const CWD_AGNOSTIC_HELPERS = new Set(["forge_do", "forge_exec_tool", "forge_state_file"]);

/**
 * Checks that may run inside workspace packages: those that run a project tool
 * (which reads its config from the working directory) and no engine helper other
 * than the cwd-agnostic ones (the others work on repository paths: ratchet,
 * migrations, Docker).
 */
function perPackage(t: CheckTarget): boolean {
  const helpers =
    commandsOf(t)
      .join("\n")
      .match(/\bforge_[a-z_]+/g) ?? [];
  return helpers.every((h) => CWD_AGNOSTIC_HELPERS.has(h)) && checkTools(t).some((x) => (PROJECT_TOOLS as readonly string[]).includes(x));
}

/** The repository a plan places rules in: its facts, workspace packages, active packs and tools. */
export interface Site {
  packs: Map<string, Pack>;
  facts?: ProjectFacts;
  workspaces: WorkspaceFacts[];
  active: PackStatus[];
  /** Where each audited tool is configured (project level). */
  places?: Record<AuditedTool, string[]>;
  /** Whether each audited tool is configured anywhere. */
  tools: Record<AuditedTool, boolean>;
}

export function site(level: Level, base: string, packs: Map<string, Pack>, facts: ProjectFacts | undefined, active: PackStatus[]): Site {
  const places = level === "project" && facts ? toolPlaces(base, facts) : undefined;
  const tools = Object.fromEntries(AUDITED_TOOLS.map((t) => [t, (places?.[t].length ?? 0) > 0])) as Record<AuditedTool, boolean>;
  return { packs, facts, workspaces: level === "project" ? (facts?.workspaces ?? []) : [], active, places, tools };
}

/** Where each audited tool's audits run: its places, or the root when it has none. */
export function toolDirs(s: Site): Record<AuditedTool, string[]> {
  return Object.fromEntries(AUDITED_TOOLS.map((t) => [t, s.places?.[t].length ? s.places[t] : [""]])) as Record<AuditedTool, string[]>;
}

/**
 * The workspace packages where a pack was detected, or undefined when it applies to
 * the whole repository (detected at the root, added in forge.yaml, required, a
 * parent of an added pack, or a local rule's pack outside the tree).
 */
export function packDirs(s: Site, id: string): string[] | undefined {
  const pack = s.packs.get(id);
  if (!pack || !s.facts || !s.workspaces.length || s.active.find((p) => p.id === id)?.why !== "detected" || detectPack(pack, s.facts.root)) return undefined;
  return s.workspaces.filter((w) => detectPack(pack, w)).map((w) => w.dir);
}

/** Whether every dependency of the rule's version gate (`when`) is declared in its range. */
function gateHolds(rule: Rule, facts: DepFacts): boolean {
  return (rule.when?.deps ?? []).every((d) => depMatches(facts, d));
}

/**
 * Where a rule applies: its pack's places (packDirs), narrowed to those where its
 * version gate holds. A pack that applies to the whole repository is judged at the
 * root, else (added in forge.yaml, required) in any workspace package. [] : the gate
 * holds nowhere.
 */
export function ruleDirs(s: Site, rule: Rule): string[] | undefined {
  const where = packDirs(s, rule.pack);
  if (!rule.when || !s.facts) return where;
  if (where) return s.workspaces.filter((w) => where.includes(w.dir) && gateHolds(rule, w)).map((w) => w.dir);
  return gateHolds(rule, s.facts.root) ? undefined : s.workspaces.filter((w) => gateHolds(rule, w)).map((w) => w.dir);
}

/** Whether the rule's version gate leaves it out everywhere. */
export function gatedOut(s: Site, rule: Rule): boolean {
  if (!rule.when || !s.facts) return false;
  return ruleDirs(s, rule)?.length === 0;
}

/**
 * The packages an ast-grep target with a where needs (type stubs) that are not declared
 * where the rule applies: at the root (or in any package) for a repository-wide rule,
 * in each of its packages otherwise.
 */
export function missingNeeds(s: Site, rule: Rule, t: AstGrepTarget): string[] {
  if (!t.needs || !s.facts) return t.needs && !s.facts ? t.needs.map((n) => n.name) : [];
  const dirs = ruleDirs(s, rule);
  const places: DepFacts[] = dirs ? s.workspaces.filter((w) => dirs.includes(w.dir)) : [s.facts.root, ...s.workspaces];
  return t.needs.filter((n) => (dirs ? !places.every((f) => depMatches(f, n)) : !places.some((f) => depMatches(f, n)))).map((n) => n.name);
}

/**
 * Where a check runs: at the root (dirs undefined) when its pack is there and the
 * root configures its audited tools; otherwise in each workspace package that has
 * its pack (or any package, for a pack active at the root) and configures them.
 * `absent`: the audited tools it needs that are configured nowhere it could run.
 */
export function checkPlace(s: Site, rule: Rule, t: CheckTarget): { dirs?: string[]; absent: string[] } {
  const audited = checkTools(t).filter((x): x is AuditedTool => (AUDITED_TOOLS as readonly string[]).includes(x));
  const absent = audited.filter((x) => !s.tools[x]);
  const places = s.places;
  if (absent.length || !places || !s.facts || !s.workspaces.length || !perPackage(t)) return { absent };
  const where = ruleDirs(s, rule);
  if (!where && audited.every((x) => places[x].includes(""))) return { absent: [] };
  const dirs = s.workspaces.filter((w) => (!where || where.includes(w.dir)) && audited.every((x) => places[x].includes(w.dir))).map((w) => w.dir);
  return dirs.length ? { dirs, absent: [] } : { absent: audited };
}
