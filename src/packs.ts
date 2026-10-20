import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { satisfies, minVersion, coerce, validRange } from "semver";
import { parse as parseJsonc } from "jsonc-parser";
import { parse as parseToml } from "smol-toml";
import { parseStrictYaml } from "./yaml.js";
import { validate } from "./schemas.js";
import { sha256 } from "./util.js";
import type { DepSpec, Pack, ProjectConfig, Rule } from "./types.js";

export function loadRuleFile(file: string, origin: Rule["origin"]): Rule {
  const text = readFileSync(file, "utf8");
  const data = parseStrictYaml(text, file) as Record<string, unknown>;
  validate("rule", data, file);
  const rule = data as unknown as Rule;
  return { ...rule, file, hash: sha256(text), origin, pack: rule.id.split("/")[0]! };
}

export function loadPacks(packsDir: string): Map<string, Pack> {
  const packs = new Map<string, Pack>();
  for (const name of readdirSync(packsDir).sort()) {
    const dir = join(packsDir, name);
    if (!statSync(dir).isDirectory()) continue;
    const packFile = join(dir, "pack.yaml");
    const data = parseStrictYaml(readFileSync(packFile, "utf8"), packFile) as Record<string, unknown>;
    validate("pack", data, packFile);
    const pack = data as unknown as Pack;
    if (pack.id !== name) throw new Error(`${packFile}: id "${pack.id}" must equal the directory name "${name}"`);
    const rulesDir = join(dir, "rules");
    const rules: Rule[] = [];
    if (existsSync(rulesDir)) {
      for (const f of readdirSync(rulesDir).sort()) {
        if (!f.endsWith(".yaml")) continue;
        const rule = loadRuleFile(join(rulesDir, f), "pack");
        if (rule.id !== `${name}/${f.slice(0, -5)}`) throw new Error(`${rule.file}: id must be "${name}/${f.slice(0, -5)}"`);
        rules.push(rule);
      }
    }
    const contextFile = join(dir, "context.md");
    const initDir = join(dir, "init");
    packs.set(name, {
      ...pack,
      requires: pack.requires ?? [],
      dir,
      context: existsSync(contextFile) ? readFileSync(contextFile, "utf8") : undefined,
      initDir: existsSync(initDir) ? initDir : undefined,
      rules,
    });
  }
  for (const pack of packs.values())
    for (const req of pack.requires) if (!packs.has(req)) throw new Error(`pack ${pack.id}: requires unknown pack ${req}`);
  return packs;
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

export interface ProjectFacts {
  /** name -> resolved or minimum version (or "" when unknown). */
  npm: Map<string, string>;
  pypi: Map<string, string>;
  files: (glob: string) => boolean;
}

function normPy(name: string): string {
  return name.toLowerCase().replace(/[-_.]+/g, "-");
}

export function readProjectFacts(repo: string, listFiles: () => string[]): ProjectFacts {
  const npm = new Map<string, string>();
  const pypi = new Map<string, string>();

  const pkgFile = join(repo, "package.json");
  if (existsSync(pkgFile)) {
    const pkg = JSON.parse(readFileSync(pkgFile, "utf8")) as Record<string, Record<string, string> | undefined>;
    for (const key of ["dependencies", "devDependencies", "optionalDependencies", "peerDependencies"])
      for (const [name, range] of Object.entries(pkg[key] ?? {})) npm.set(name, versionFromRange(range));
  }
  const bunLock = join(repo, "bun.lock");
  if (existsSync(bunLock)) {
    const lock = parseJsonc(readFileSync(bunLock, "utf8"), [], { allowTrailingComma: true }) as {
      packages?: Record<string, unknown[]>;
    };
    for (const [name, entry] of Object.entries(lock.packages ?? {})) {
      if (name.includes("/") && !name.startsWith("@")) continue; // nested resolution
      const spec = Array.isArray(entry) && typeof entry[0] === "string" ? entry[0] : "";
      const at = spec.lastIndexOf("@");
      if (at > 0 && npm.has(name)) npm.set(name, spec.slice(at + 1));
    }
  }

  const pyproject = join(repo, "pyproject.toml");
  if (existsSync(pyproject)) {
    const toml = parseToml(readFileSync(pyproject, "utf8")) as Record<string, any>;
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
  const uvLock = join(repo, "uv.lock");
  if (existsSync(uvLock)) {
    const lock = parseToml(readFileSync(uvLock, "utf8")) as { package?: { name: string; version?: string }[] };
    for (const p of lock.package ?? []) {
      const n = normPy(p.name);
      if (pypi.has(n) && p.version) pypi.set(n, p.version);
    }
  }
  const files = listFiles();
  return { npm, pypi, files: (glob) => files.some((f) => globMatch(glob, f)) };
}

function versionFromRange(range: string): string {
  if (!validRange(range)) return "";
  return minVersion(range)?.version ?? "";
}

function versionFromPep440(spec: string): string {
  const m = /(?:==|>=|~=|===)\s*([0-9][0-9A-Za-z.]*)/.exec(spec);
  return m ? (coerce(m[1]!)?.version ?? "") : "";
}

function depMatches(facts: ProjectFacts, dep: DepSpec): boolean {
  const map = dep.ecosystem === "npm" ? facts.npm : facts.pypi;
  const name = dep.ecosystem === "pypi" ? normPy(dep.name) : dep.name;
  if (!map.has(name)) return false;
  if (!dep.version) return true;
  const v = coerce(map.get(name)!)?.version;
  // An unknown version (e.g. "workspace:*", git URL) is accepted.
  return !v || satisfies(v, dep.version, { includePrerelease: true });
}

export function detectPack(pack: Pack, facts: ProjectFacts): boolean {
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
