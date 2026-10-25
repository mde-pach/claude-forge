import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { satisfies, minVersion, coerce, validRange } from "semver";
import { parse as parseJsonc } from "jsonc-parser/lib/esm/main.js";
import { parse as parseToml } from "smol-toml";
import { parse as parseYaml } from "yaml";
import { parseStrictYaml } from "./yaml.js";
import { validate } from "./schemas.js";
import { sha256 } from "./util.js";
import type { DepSpec, Pack, ProjectConfig, Rule } from "./types.js";

/** A rule's code examples (strings); fixtures are for check rules only. */
export function codeExamples(rule: Pick<Rule, "examples">, kind: "match" | "no_match"): string[] {
  return (rule.examples?.[kind] ?? []).filter((e): e is string => typeof e === "string");
}

export function loadRuleFile(file: string, origin: Rule["origin"]): Rule {
  const text = readFileSync(file, "utf8");
  const data = parseStrictYaml(text, file) as Record<string, unknown>;
  validate("rule", data, file);
  const rule = data as unknown as Rule;
  return { ...rule, file, hash: sha256(text), origin, pack: rule.id.slice(0, rule.id.lastIndexOf("/")) };
}

/** Directory names inside a pack that are not child packs. */
const RESERVED = new Set(["rules", "init"]);

/**
 * Loads the pack tree. A directory with a pack.yaml is a pack whose id is its
 * path below packsDir; its sub-directories with a pack.yaml are its children.
 */
export function loadPacks(packsDir: string): Map<string, Pack> {
  const packs = new Map<string, Pack>();
  const walk = (dir: string, id: string, parent?: Pack) => {
    const packFile = join(dir, "pack.yaml");
    const data = parseStrictYaml(readFileSync(packFile, "utf8"), packFile) as Record<string, unknown>;
    validate("pack", data, packFile);
    const pack = data as unknown as Pack;
    if (pack.id !== id) throw new Error(`${packFile}: id "${pack.id}" must equal its path in the pack tree, "${id}"`);
    if (parent && pack.level !== parent.level) throw new Error(`${packFile}: level ${pack.level} differs from parent ${parent.id} (${parent.level})`);
    const rulesDir = join(dir, "rules");
    const rules: Rule[] = [];
    if (existsSync(rulesDir)) {
      for (const f of readdirSync(rulesDir).sort()) {
        if (!f.endsWith(".yaml")) continue;
        const rule = loadRuleFile(join(rulesDir, f), "pack");
        if (rule.id !== `${id}/${f.slice(0, -5)}`) throw new Error(`${rule.file}: id must be "${id}/${f.slice(0, -5)}"`);
        rules.push(rule);
      }
    }
    const contextFile = join(dir, "context.md");
    const initDir = join(dir, "init");
    const loaded: Pack = {
      ...pack,
      requires: pack.requires ?? [],
      dir,
      parent: parent?.id,
      children: [],
      context: existsSync(contextFile) ? readFileSync(contextFile, "utf8") : undefined,
      initDir: existsSync(initDir) ? initDir : undefined,
      rules,
    };
    packs.set(id, loaded);
    parent?.children.push(id);
    for (const name of readdirSync(dir).sort()) {
      const sub = join(dir, name);
      if (RESERVED.has(name) || !statSync(sub).isDirectory()) continue;
      if (existsSync(join(sub, "pack.yaml"))) walk(sub, `${id}/${name}`, loaded);
      else if (readdirSync(sub).length) throw new Error(`${sub}: not a pack (no pack.yaml)`);
    }
  };
  for (const name of readdirSync(packsDir).sort()) {
    const dir = join(packsDir, name);
    if (statSync(dir).isDirectory()) walk(dir, name);
  }
  for (const pack of packs.values())
    for (const req of pack.requires) if (!packs.has(req)) throw new Error(`pack ${pack.id}: requires unknown pack ${req}`);
  return packs;
}

/** The pack and its ancestors, root first. */
export function lineage(packs: Map<string, Pack>, id: string): string[] {
  const out: string[] = [];
  for (let p = packs.get(id); p; p = p.parent ? packs.get(p.parent) : undefined) out.unshift(p.id);
  return out;
}

export function loadProjectConfig(repo: string): ProjectConfig | undefined {
  const file = join(repo, "forge.yaml");
  if (!existsSync(file)) return undefined;
  const data = parseStrictYaml(readFileSync(file, "utf8"), file);
  validate("project", data, file);
  return data as ProjectConfig;
}

export function loadLocalRules(repo: string): Rule[] {
  const dir = join(repo, ".forge", "rules");
  if (!existsSync(dir)) return [];
  return readdirSync(dir)
    .filter((f) => f.endsWith(".yaml"))
    .sort()
    .map((f) => loadRuleFile(join(dir, f), "local"));
}

// ---------------------------------------------------------------- detection

export interface DepFacts {
  /** name -> resolved or minimum version (or "" when unknown). */
  npm: Map<string, string>;
  pypi: Map<string, string>;
  files: (glob: string) => boolean;
}

/** A workspace package: its directory (relative to the repository) and its own facts, globs relative to it. */
export interface WorkspaceFacts extends DepFacts {
  dir: string;
}

/**
 * What detection sees: the root's dependencies and files merged with those of
 * the workspace packages (a glob matches at the root or inside a package), plus
 * the root alone and each package alone, to decide where checks run.
 */
export interface ProjectFacts extends DepFacts {
  root: DepFacts;
  workspaces: WorkspaceFacts[];
}

function normPy(name: string): string {
  return name.toLowerCase().replace(/[-_.]+/g, "-");
}

function readJson(file: string): Record<string, any> | undefined {
  try {
    return JSON.parse(readFileSync(file, "utf8")) as Record<string, any>;
  } catch {
    return undefined;
  }
}

function readTomlFile(file: string): Record<string, any> | undefined {
  try {
    return parseToml(readFileSync(file, "utf8")) as Record<string, any>;
  } catch {
    return undefined;
  }
}

/** Dependencies declared by the package.json and pyproject.toml in dir (the root's must parse; a package's may not). */
function readDeps(dir: string, strict: boolean): { npm: Map<string, string>; pypi: Map<string, string> } {
  const npm = new Map<string, string>();
  const pypi = new Map<string, string>();
  const read = <T>(file: string, parse: (f: string) => T | undefined, raw: (t: string) => T) =>
    !existsSync(file) ? undefined : strict ? raw(readFileSync(file, "utf8")) : parse(file);
  const pkg = read(join(dir, "package.json"), readJson, (t) => JSON.parse(t) as Record<string, any>);
  for (const key of ["dependencies", "devDependencies", "optionalDependencies", "peerDependencies"])
    for (const [name, range] of Object.entries<unknown>(pkg?.[key] ?? {})) if (typeof range === "string") npm.set(name, versionFromRange(range));
  const toml = read(join(dir, "pyproject.toml"), readTomlFile, (t) => parseToml(t) as Record<string, any>);
  if (toml) {
    const reqs: string[] = [
      ...(toml.project?.dependencies ?? []),
      ...Object.values<string[]>(toml.project?.["optional-dependencies"] ?? {}).flat(),
      ...Object.values<unknown[]>(toml["dependency-groups"] ?? {})
        .flat()
        .filter((x): x is string => typeof x === "string"),
      ...(toml.tool?.uv?.["dev-dependencies"] ?? []),
    ];
    for (const req of reqs) {
      const m = /^\s*([A-Za-z0-9][A-Za-z0-9._-]*)\s*(?:\[[^\]]*\])?\s*(.*)$/.exec(req);
      if (m) pypi.set(normPy(m[1]!), versionFromPep440(m[2]!));
    }
  }
  return { npm, pypi };
}

/**
 * The workspace packages of a monorepo, as directories relative to the root:
 * pnpm-workspace.yaml `packages`, package.json `workspaces` (npm, Yarn, Bun) and
 * `[tool.uv.workspace] members`, with their `!`/`exclude` patterns; without any of
 * these, `packages/*`. A package is a directory with a package.json or pyproject.toml.
 */
export function workspaceDirs(repo: string, files: string[]): string[] {
  const include: string[] = [];
  const exclude: string[] = [];
  const add = (globs: unknown) => {
    if (!Array.isArray(globs)) return;
    for (const g of globs)
      if (typeof g === "string") (g.startsWith("!") ? exclude : include).push(g.replace(/^!/, "").replace(/^\.\//, "").replace(/\/+$/, ""));
  };
  const pnpm = join(repo, "pnpm-workspace.yaml");
  if (existsSync(pnpm)) {
    try {
      add((parseYaml(readFileSync(pnpm, "utf8")) as { packages?: unknown } | null)?.packages);
    } catch {
      // an unreadable workspace file declares nothing
    }
  }
  const pkg = existsSync(join(repo, "package.json")) ? readJson(join(repo, "package.json")) : undefined;
  add(Array.isArray(pkg?.workspaces) ? pkg.workspaces : pkg?.workspaces?.packages);
  const uv = existsSync(join(repo, "pyproject.toml")) ? readTomlFile(join(repo, "pyproject.toml"))?.tool?.uv?.workspace : undefined;
  add(uv?.members);
  if (Array.isArray(uv?.exclude)) for (const g of uv.exclude) if (typeof g === "string") exclude.push(g.replace(/^\.\//, "").replace(/\/+$/, ""));
  if (!include.length) include.push("packages/*");
  const dirs = new Set<string>();
  for (const f of files) {
    const m = /^(.+)\/(?:package\.json|pyproject\.toml)$/.exec(f);
    // check.sh lists package directories in a word list: names with spaces or quotes are left out
    if (!m || /(^|\/)node_modules\//.test(f) || !/^[A-Za-z0-9._@+\/-]+$/.test(m[1]!)) continue;
    const dir = m[1]!;
    if (include.some((g) => globMatch(g, dir)) && !exclude.some((g) => globMatch(g, dir))) dirs.add(dir);
  }
  return [...dirs].sort();
}

export function readProjectFacts(repo: string, listFiles: () => string[]): ProjectFacts {
  const { npm, pypi } = readDeps(repo, true);
  const bunLock = join(repo, "bun.lock");
  const lockNpm = new Map<string, string>();
  if (existsSync(bunLock)) {
    const lock = parseJsonc(readFileSync(bunLock, "utf8"), [], { allowTrailingComma: true }) as {
      packages?: Record<string, unknown[]>;
    };
    for (const [name, entry] of Object.entries(lock.packages ?? {})) {
      if (name.includes("/") && !name.startsWith("@")) continue; // nested resolution
      const spec = Array.isArray(entry) && typeof entry[0] === "string" ? entry[0] : "";
      const at = spec.lastIndexOf("@");
      if (at > 0) lockNpm.set(name, spec.slice(at + 1));
    }
  }
  const uvLock = join(repo, "uv.lock");
  const lockPypi = new Map<string, string>();
  if (existsSync(uvLock)) {
    const lock = parseToml(readFileSync(uvLock, "utf8")) as { package?: { name: string; version?: string }[] };
    for (const p of lock.package ?? []) if (p.version) lockPypi.set(normPy(p.name), p.version);
  }
  // the lock files at the root resolve the versions of the root and of every workspace package
  const resolve = (deps: { npm: Map<string, string>; pypi: Map<string, string> }) => {
    for (const name of deps.npm.keys()) if (lockNpm.has(name)) deps.npm.set(name, lockNpm.get(name)!);
    for (const name of deps.pypi.keys()) if (lockPypi.has(name)) deps.pypi.set(name, lockPypi.get(name)!);
    return deps;
  };
  resolve({ npm, pypi });
  const files = listFiles();
  const matcher = (prefix: string) => {
    const mine = prefix ? files.filter((f) => f.startsWith(prefix + "/")).map((f) => f.slice(prefix.length + 1)) : files;
    return (glob: string) => mine.some((f) => globMatch(glob, f));
  };
  const root: DepFacts = { npm, pypi, files: matcher("") };
  const workspaces: WorkspaceFacts[] = workspaceDirs(repo, files).map((dir) => ({ dir, ...resolve(readDeps(join(repo, dir), false)), files: matcher(dir) }));
  const merged = <T>(pick: (d: DepFacts) => Map<string, T>) => {
    const m = new Map<string, T>();
    for (const d of [...workspaces].reverse()) for (const [k, v] of pick(d)) m.set(k, v);
    for (const [k, v] of pick(root)) m.set(k, v); // the root's version wins
    return m;
  };
  return {
    npm: merged((d) => d.npm),
    pypi: merged((d) => d.pypi),
    files: (glob) => root.files(glob) || workspaces.some((w) => w.files(glob)),
    root,
    workspaces,
  };
}

function versionFromRange(range: string): string {
  if (!validRange(range)) return "";
  return minVersion(range)?.version ?? "";
}

function versionFromPep440(spec: string): string {
  const m = /(?:==|>=|~=|===)\s*([0-9][0-9A-Za-z.]*)/.exec(spec);
  return m ? (coerce(m[1]!)?.version ?? "") : "";
}

function depMatches(facts: DepFacts, dep: DepSpec): boolean {
  const map = dep.ecosystem === "npm" ? facts.npm : facts.pypi;
  const name = dep.ecosystem === "pypi" ? normPy(dep.name) : dep.name;
  if (!map.has(name)) return false;
  if (!dep.version) return true;
  const v = coerce(map.get(name)!)?.version;
  // An unknown version (e.g. "workspace:*", git URL) is accepted.
  return !v || satisfies(v, dep.version, { includePrerelease: true });
}

export function detectPack(pack: Pack, facts: DepFacts): boolean {
  if (!pack.detect) return false;
  return (
    (pack.detect.files_any ?? []).some((g) => facts.files(g)) ||
    (pack.detect.deps_any ?? []).some((d) => depMatches(facts, d))
  );
}

// ---------------------------------------------------------------- globs

const globCache = new Map<string, RegExp>();

/** Glob to RegExp: `**` spans directories, `*` and `?` stay within one segment, `{a,b}` alternation. */
export function globToRegExp(glob: string): RegExp {
  let re = globCache.get(glob);
  if (re) return re;
  let out = "";
  for (let i = 0; i < glob.length; i++) {
    const c = glob[i]!;
    if (c === "*") {
      if (glob[i + 1] === "*") {
        i++;
        if (glob[i + 1] === "/") {
          i++;
          out += "(?:.*/)?";
        } else out += ".*";
      } else out += "[^/]*";
    } else if (c === "?") out += "[^/]";
    else if (c === "{") {
      const end = glob.indexOf("}", i);
      out += `(?:${glob
        .slice(i + 1, end)
        .split(",")
        .map((s) => s.replace(/[.+^$()|[\]\\]/g, "\\$&").replace(/\*/g, "[^/]*"))
        .join("|")})`;
      i = end;
    } else out += c.replace(/[.+^$()|[\]\\]/g, "\\$&");
  }
  re = new RegExp(`^${out}$`);
  globCache.set(glob, re);
  return re;
}

export function globMatch(glob: string, path: string): boolean {
  return globToRegExp(glob).test(path);
}
