import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { satisfies, minVersion, coerce, validRange } from "semver";
import { parse as parseJsonc } from "jsonc-parser/lib/esm/main.js";
import { parse as parseToml } from "smol-toml";
import { parse as parseYaml } from "yaml";
import { parseStrictYaml } from "./yaml.js";
import { validate } from "./schemas.js";
import { sha256 } from "./util.js";
import type { DepSpec, GuardExample, Pack, ProjectConfig, Rule } from "./types.js";

/** A rule's code examples (strings); fixtures are for check rules only. */
export function codeExamples(rule: Pick<Rule, "examples">, kind: "match" | "no_match"): string[] {
  return (rule.examples?.[kind] ?? []).filter((e): e is string => typeof e === "string");
}

/** A guard rule's examples as commands, each with the files of the directory it runs from. */
export function guardExamples(rule: Pick<Rule, "examples">, kind: "match" | "no_match"): GuardExample[] {
  return (rule.examples?.[kind] ?? []).flatMap((e) => (typeof e === "string" ? [{ command: e }] : "command" in e ? [e] : []));
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
  /** Ecosystems the project publishes a library to (see isPyLibrary, isNpmLibrary). */
  library: Set<"pypi" | "npm">;
}

/** A workspace package: its directory (relative to the repository) and its own facts, globs relative to it. */
export interface WorkspaceFacts extends DepFacts {
  dir: string;
}

/**
 * What detection sees: the root and each workspace package, each with its own
 * dependencies (never merged: next@14 in one package and next@16 in another are
 * two facts) and files. A pack is detected when it is detected in one of them.
 */
export interface ProjectFacts {
  root: DepFacts;
  workspaces: WorkspaceFacts[];
}

/**
 * A Python library: pyproject.toml names the project and declares a build backend
 * (uv init --lib; an application from uv init has no [build-system]), ships no
 * console or GUI scripts (uv init --package: a packaged application), is not marked
 * "Private :: Do Not Upload", its directory holds no manage.py (a Django site), and
 * there is evidence it is published: trove classifiers (PyPI's metadata, which an
 * internal service has no use for) or a CI workflow that uploads to PyPI. A build
 * backend alone is not enough: uv workspace members and packaged services have one.
 */
export function isPyLibrary(toml: Record<string, any>, hasManagePy: boolean, publishedByCi = false): boolean {
  const project = toml.project;
  if (!project || typeof project !== "object" || typeof project.name !== "string" || !toml["build-system"]) return false;
  if (project.scripts || project["gui-scripts"] || hasManagePy) return false;
  const classifiers = Array.isArray(project.classifiers) ? project.classifiers.filter((c: unknown): c is string => typeof c === "string") : [];
  if (classifiers.some((c: string) => c.startsWith("Private ::"))) return false;
  return publishedByCi || classifiers.length > 0;
}

/**
 * An npm library: package.json is not "private": true, has a name, declares what
 * it exports ("exports", or "main" / "module" together with "types", "typings" or
 * "files"; "main" alone is npm init's default, also in applications), and there is
 * evidence it is published: "publishConfig", or a release set-up that publishes
 * (Changesets, a CI workflow running npm/pnpm/yarn/bun publish or semantic-release).
 * Internal workspace packages export too, but are not published.
 */
export function isNpmLibrary(pkg: Record<string, unknown>, publishedByCi = false): boolean {
  if (pkg.private === true || typeof pkg.name !== "string") return false;
  const exportsApi = pkg.exports !== undefined || ((pkg.main !== undefined || pkg.module !== undefined) && (pkg.types !== undefined || pkg.typings !== undefined || pkg.files !== undefined));
  return exportsApi && (publishedByCi || (pkg.publishConfig !== undefined && pkg.publishConfig !== null));
}

/** Release set-ups that publish to PyPI or npm, found in CI workflows (GitHub Actions, GitLab CI) and Changesets. */
export function publishSignals(repo: string, files: string[]): { pypi: boolean; npm: boolean } {
  const ci = files.filter((f) => /^\.github\/workflows\/[^/]+\.ya?ml$/.test(f) || f === ".gitlab-ci.yml");
  let text = "";
  for (const f of ci) {
    try {
      text += readFileSync(join(repo, f), "utf8") + "\n";
    } catch {
      // unreadable: no evidence
    }
  }
  return {
    pypi: /pypa\/gh-action-pypi-publish|\b(uv|poetry|hatch|flit|pdm)\s+publish\b|\btwine\s+upload\b/.test(text),
    npm: files.includes(".changeset/config.json") || /\b(npm|pnpm|bun)\s+publish\b|\byarn\s+(npm\s+)?publish\b|changesets\/action|semantic-release|JS-DevTools\/npm-publish/.test(text),
  };
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
function readDeps(dir: string, strict: boolean, hasManagePy = false, published = { pypi: false, npm: false }): Omit<DepFacts, "files"> {
  const npm = new Map<string, string>();
  const pypi = new Map<string, string>();
  const library = new Set<"pypi" | "npm">();
  const read = <T>(file: string, parse: (f: string) => T | undefined, raw: (t: string) => T) =>
    !existsSync(file) ? undefined : strict ? raw(readFileSync(file, "utf8")) : parse(file);
  const pkg = read(join(dir, "package.json"), readJson, (t) => JSON.parse(t) as Record<string, any>);
  for (const key of ["dependencies", "devDependencies", "optionalDependencies", "peerDependencies"])
    for (const [name, range] of Object.entries<unknown>(pkg?.[key] ?? {})) if (typeof range === "string") npm.set(name, versionFromRange(range));
  if (pkg && isNpmLibrary(pkg, published.npm)) library.add("npm");
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
    if (isPyLibrary(toml, hasManagePy, published.pypi)) library.add("pypi");
  }
  return { npm, pypi, library };
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
  // Lerna declares its packages in lerna.json (default packages/*); without any declaration the repository has none
  const lerna = existsSync(join(repo, "lerna.json")) ? readJson(join(repo, "lerna.json")) : undefined;
  if (lerna) add(Array.isArray(lerna.packages) ? lerna.packages : ["packages/*"]);
  const dirs = new Set<string>();
  for (const f of files) {
    const m = /^(.+)\/(?:package\.json|pyproject\.toml)$/.exec(f);
    // check.sh lists package directories in a word list: names with spaces or quotes are left out
    if (!m || /(^|\/)node_modules\//.test(f) || !/^[A-Za-z0-9._@+\/-]+$/.test(m[1]!)) continue;
    const dir = m[1]!;
    // an exclusion like "**/test/**" also excludes the test directory itself
    if (include.some((g) => globMatch(g, dir)) && !exclude.some((g) => globMatch(g, dir) || globMatch(g, dir + "/"))) dirs.add(dir);
  }
  return [...dirs].sort();
}

/**
 * Each file under the workspace packages holding it (nested packages: each of them), relative
 * to the package, and under "" the files outside every package; one pass over the file list.
 */
function filesByPackage(files: string[], dirs: string[]): Map<string, string[]> {
  const out = new Map<string, string[]>([["", []], ...dirs.map((d): [string, string[]] => [d, []])]);
  const set = new Set(dirs);
  for (const f of files) {
    let inPackage = false;
    for (let i = f.indexOf("/"); i > 0; i = f.indexOf("/", i + 1)) {
      const d = f.slice(0, i);
      if (set.has(d)) {
        out.get(d)!.push(f.slice(i + 1));
        inPackage = true;
      }
    }
    if (!inPackage) out.get("")!.push(f);
  }
  return out;
}

export function readProjectFacts(repo: string, listFiles: () => string[]): ProjectFacts {
  const files = listFiles();
  const wsDirs = workspaceDirs(repo, files);
  const own = filesByPackage(files, wsDirs);
  // a manage.py makes its own package a Django site (the root: one outside the workspace packages)
  const hasManagePy = (dir: string) => own.get(dir)!.some((f) => f === "manage.py" || f.endsWith("/manage.py"));
  const published = publishSignals(repo, files);
  const { npm, pypi, library } = readDeps(repo, true, hasManagePy(""), published);
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
  // the root's files are those outside the workspace packages: a file belongs to its package
  const matcher = (dir: string) => {
    const mine = own.get(dir)!;
    const seen = new Map<string, boolean>();
    return (glob: string) => {
      let hit = seen.get(glob);
      if (hit === undefined) {
        const re = globToRegExp(glob);
        hit = mine.some((f) => re.test(f));
        seen.set(glob, hit);
      }
      return hit;
    };
  };
  const root: DepFacts = { npm, pypi, library, files: matcher("") };
  const workspaces: WorkspaceFacts[] = wsDirs.map((dir) => {
    const d = readDeps(join(repo, dir), false, hasManagePy(dir), published);
    return { dir, ...resolve(d), library: d.library, files: matcher(dir) };
  });
  return { root, workspaces };
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

/** Whether the pack is detected at the root or in a workspace package (each judged on its own facts). */
export function detectAnywhere(pack: Pack, facts: ProjectFacts): boolean {
  return detectPack(pack, facts.root) || facts.workspaces.some((w) => detectPack(pack, w));
}

export function detectPack(pack: Pack, facts: DepFacts): boolean {
  if (!pack.detect) return false;
  return (
    (pack.detect.files_any ?? []).some((g) => facts.files(g)) ||
    (pack.detect.deps_any ?? []).some((d) => depMatches(facts, d)) ||
    (pack.detect.library_any ?? []).some((e) => facts.library.has(e))
  );
}

// ---------------------------------------------------------------- globs

const globCache = new Map<string, RegExp>();

/** Glob to RegExp: `**` spans directories, `*` and `?` stay within one segment, `{a,b}` alternation (nested, and holding any of these). */
export function globToRegExp(glob: string): RegExp {
  let re = globCache.get(glob);
  if (re) return re;
  re = new RegExp(`^${globSource(glob)}$`);
  globCache.set(glob, re);
  return re;
}

function globSource(glob: string): string {
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
      // the matching brace, and the alternatives split at depth 0
      const alts: string[] = [];
      let depth = 0;
      let start = i + 1;
      let end = -1;
      for (let k = i + 1; k < glob.length && end < 0; k++) {
        if (glob[k] === "{") depth++;
        else if (glob[k] === "}" && depth > 0) depth--;
        else if (glob[k] === "}") end = k;
        else if (glob[k] === "," && depth === 0) {
          alts.push(glob.slice(start, k));
          start = k + 1;
        }
      }
      if (end < 0) {
        out += "\\{";
        continue;
      }
      alts.push(glob.slice(start, end));
      out += `(?:${alts.map(globSource).join("|")})`;
      i = end;
    } else out += c.replace(/[.+^$()|[\]\\]/g, "\\$&");
  }
  return out;
}

export function globMatch(glob: string, path: string): boolean {
  return globToRegExp(glob).test(path);
}
