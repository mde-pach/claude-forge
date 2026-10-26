import { existsSync, readFileSync } from "node:fs";
import { join, posix } from "node:path";
import { parse as parseToml } from "smol-toml";
import type { ProjectConfig, Rule, TypeSpec, WhereCondition } from "./types.js";

/**
 * File roles (§15.4): rules and helpers name the files they are about by role
 * (`@python-tests`), never by literal globs. A role's effective patterns are Forge's
 * defaults, replaced by what the tool's own declarative config says, then changed by
 * forge.yaml (`files: {<role>: {set | add}}`). `forge apply` renders them literally into
 * the generated files; hooks never read configs.
 */

const NAME = "[a-z][a-z0-9-]*";
/** `@role` or `!@role` as an entry of a glob list (scope.files, when_changed, a role's patterns). */
const REF = new RegExp(`^(!?)@(${NAME})$`);
/** `@role` as a word of a check command (forge_role @python-tests "$f"). */
const COMMAND_REF = new RegExp(`(?:^|[\\s'"])@(${NAME})(?=$|[\\s'";|&)])`, "g");
/** `@role` as a where module (a role whose files declare the type). */
const MODULE_REF = new RegExp(`^@(${NAME})$`);

export const FORGE_DEFAULT = "forge default";
export const FORGE_YAML = "forge.yaml";

/** A pattern of a role and where it comes from: "forge default", the tool's config file, or "forge.yaml". */
export interface RolePattern {
  glob: string;
  exclude: boolean;
  source: string;
}
export interface EffectiveRole {
  patterns: RolePattern[];
  /** What the tool's config said that Forge could not use (reported by forge status, never guessed). */
  notes: string[];
}
export type Roles = Map<string, EffectiveRole>;

/** What role defaults read: the repository and its files (tracked and untracked, not ignored). */
export interface RoleInputs {
  repo: string;
  files: string[];
}

interface Part {
  patterns: string[];
  source: string;
}
interface RoleDef {
  /** What the role is, for status and docs. */
  about: string;
  defaults: string[];
  /** The tool's declarative config: parts that replace the defaults (all of them, or the part of one tool). */
  config?: (inputs: RoleInputs, notes: string[]) => Part[] | undefined;
}

// ---------------------------------------------------------------- tool configs (data only, never code)

function readText(inputs: RoleInputs, rel: string): string | undefined {
  const f = join(inputs.repo, rel);
  try {
    return existsSync(f) ? readFileSync(f, "utf8") : undefined;
  } catch {
    return undefined;
  }
}

function readTomlAt(inputs: RoleInputs, rel: string): Record<string, any> | undefined {
  const text = readText(inputs, rel);
  if (text === undefined) return undefined;
  try {
    return parseToml(text) as Record<string, any>;
  } catch {
    return undefined;
  }
}

function readJsonAt(inputs: RoleInputs, rel: string): Record<string, any> | undefined {
  const text = readText(inputs, rel);
  if (text === undefined) return undefined;
  try {
    return JSON.parse(text) as Record<string, any>;
  } catch {
    return undefined;
  }
}

/** An INI file's sections: name -> key -> value (continuation lines joined with a space). */
export function parseIni(text: string): Map<string, Map<string, string>> {
  const out = new Map<string, Map<string, string>>();
  let section: Map<string, string> | undefined;
  let last: string | undefined;
  for (const raw of text.split(/\r?\n/)) {
    if (/^\s*([#;]|$)/.test(raw)) continue;
    const head = /^\s*\[([^\]]+)\]\s*$/.exec(raw);
    if (head) {
      section = out.get(head[1]!.trim()) ?? new Map();
      out.set(head[1]!.trim(), section);
      last = undefined;
      continue;
    }
    if (!section) continue;
    if (/^\s/.test(raw) && last) {
      section.set(last, `${section.get(last)} ${raw.trim()}`.trim());
      continue;
    }
    const kv = /^([^=:]+?)\s*[=:]\s*(.*)$/.exec(raw);
    if (kv) {
      last = kv[1]!.trim();
      section.set(last, kv[2]!.trim());
    }
  }
  return out;
}

/** A path from a config file, relative to the repository: "./x/", "%(here)s/x" -> "x". */
function cleanPath(p: string, base = ""): string | undefined {
  const s = p.trim().replace(/^%\(here\)s[\\/]?/, "").replace(/\\/g, "/");
  if (!s || s.startsWith("/") || /^[A-Za-z]:/.test(s) || s.includes("%(")) return undefined;
  const joined = posix.normalize(posix.join(base, s)).replace(/\/+$/, "");
  return joined === "." ? "" : joined.startsWith("../") || joined === ".." ? undefined : joined;
}

const under = (dir: string, glob: string) => (dir ? `${dir}/${glob}` : glob);
const list = (v: unknown): string[] => (Array.isArray(v) ? v.filter((x): x is string => typeof x === "string") : typeof v === "string" ? v.split(/\s+/).filter(Boolean) : []);

/** pytest's ini options: pytest.ini, pyproject.toml ([tool.pytest] or [tool.pytest.ini_options]), tox.ini, setup.cfg; pytest's own order. */
function pytestOptions(inputs: RoleInputs): { options: Record<string, unknown>; source: string } | undefined {
  const ini = readText(inputs, "pytest.ini");
  if (ini !== undefined) return { options: Object.fromEntries(parseIni(ini).get("pytest") ?? []), source: "pytest.ini" };
  const py = readTomlAt(inputs, "pyproject.toml")?.tool?.pytest;
  if (py && typeof py === "object") {
    const native = Object.fromEntries(Object.entries(py).filter(([k]) => k !== "ini_options"));
    return { options: { ...(py.ini_options ?? {}), ...native }, source: "pyproject.toml" };
  }
  for (const [file, section] of [["tox.ini", "pytest"], ["setup.cfg", "tool:pytest"]] as const) {
    const s = parseIni(readText(inputs, file) ?? "").get(section);
    if (s) return { options: Object.fromEntries(s), source: file };
  }
  return undefined;
}

function pythonTests(inputs: RoleInputs): Part[] | undefined {
  const cfg = pytestOptions(inputs);
  if (!cfg) return undefined;
  const testpaths = list(cfg.options.testpaths).map((p) => cleanPath(p)).filter((p): p is string => p !== undefined);
  const pyFiles = list(cfg.options.python_files);
  if (!testpaths.length && !pyFiles.length) return undefined;
  const names = pyFiles.length ? pyFiles : ["test_*.py", "*_test.py"];
  const dirs = testpaths.length ? testpaths : [""];
  // conftest.py is pytest's own file name, wherever it is
  return [{ patterns: [...dirs.flatMap((d) => names.map((n) => under(d, `**/${n}`))), "**/conftest.py"], source: cfg.source }];
}

/** Alembic: script_location (alembic.ini [alembic], pyproject.toml [tool.alembic]) + /versions, or version_locations. */
function alembicMigrations(inputs: RoleInputs, notes: string[]): Part[] | undefined {
  const fromIni = parseIni(readText(inputs, "alembic.ini") ?? "").get("alembic");
  const fromToml = readTomlAt(inputs, "pyproject.toml")?.tool?.alembic;
  const cfg = fromIni ? { get: (k: string) => fromIni.get(k), source: "alembic.ini" } : fromToml ? { get: (k: string) => fromToml[k], source: "pyproject.toml" } : undefined;
  if (!cfg) return undefined;
  const versions = cfg.get("version_locations");
  if (versions) {
    const sep = cfg.source === "alembic.ini" ? (fromIni!.get("path_separator") ?? fromIni!.get("version_path_separator") ?? "space") : "space";
    const parts = Array.isArray(versions) ? versions : String(versions).split(sep === "space" || sep === "os" ? /\s+/ : sep === "colon" ? ":" : sep === "semicolon" ? ";" : sep === "newline" ? "\n" : ",");
    const dirs = parts.map((p: string) => cleanPath(p));
    if (dirs.every((d: string | undefined) => d !== undefined)) return [{ patterns: dirs.map((d: string | undefined) => under(d!, "**")), source: cfg.source }];
    notes.push(`${cfg.source}: version_locations ${JSON.stringify(versions)} is not a path in the repository`);
    return undefined;
  }
  const loc = cfg.get("script_location");
  if (typeof loc !== "string" || !loc) return undefined;
  const dir = loc.includes(":") && !/^%\(here\)s/.test(loc) ? undefined : cleanPath(loc);
  if (dir === undefined) {
    notes.push(`${cfg.source}: script_location ${JSON.stringify(loc)} is not a path in the repository (set files: alembic-migrations in forge.yaml)`);
    return undefined;
  }
  return [{ patterns: [under(dir, "versions/**")], source: cfg.source }];
}

/** The Prisma schema: package.json "prisma.schema" (a file or a directory), else Prisma's defaults. */
function prismaSchema(inputs: RoleInputs): { path: string; dir: string; source: string } | undefined {
  const set = readJsonAt(inputs, "package.json")?.prisma?.schema;
  if (typeof set === "string") {
    const p = cleanPath(set);
    if (p === undefined) return undefined;
    const isFile = p.endsWith(".prisma");
    return { path: p, dir: isFile ? posix.dirname(p).replace(/^\.$/, "") : p, source: "package.json" };
  }
  for (const p of ["prisma/schema.prisma", "schema.prisma"]) if (existsSync(join(inputs.repo, p))) return { path: p, dir: posix.dirname(p).replace(/^\.$/, ""), source: p };
  return undefined;
}

function prismaMigrations(inputs: RoleInputs): Part[] | undefined {
  const s = prismaSchema(inputs);
  return s?.source === "package.json" ? [{ patterns: [under(s.dir, "migrations/**")], source: "package.json" }] : undefined;
}

/** schema.prisma generator `output`s (relative to the schema file). */
function generatedClients(inputs: RoleInputs): Part[] | undefined {
  const s = prismaSchema(inputs);
  if (!s) return undefined;
  const files = s.path.endsWith(".prisma") ? [s.path] : inputs.files.filter((f) => f.startsWith(s.dir ? `${s.dir}/` : "") && f.endsWith(".prisma"));
  const out: Part[] = [];
  for (const f of files) {
    const text = readText(inputs, f) ?? "";
    const dir = posix.dirname(f).replace(/^\.$/, "");
    for (const block of text.matchAll(/^\s*generator\s+\w+\s*\{([^}]*)\}/gm)) {
      const m = /^\s*output\s*=\s*"([^"]+)"/m.exec(block[1]!);
      const p = m ? cleanPath(m[1]!, dir) : undefined;
      if (p !== undefined) out.push({ patterns: [under(p, "**")], source: f });
    }
  }
  return out.length ? out : undefined;
}

/** pytest-django's DJANGO_SETTINGS_MODULE (a dotted module: its file, or its package). */
function djangoSettings(inputs: RoleInputs): Part[] | undefined {
  const cfg = pytestOptions(inputs);
  const mod = cfg?.options.DJANGO_SETTINGS_MODULE;
  if (typeof mod !== "string" || !/^[A-Za-z_][\w.]*$/.test(mod)) return undefined;
  const p = mod.replaceAll(".", "/");
  return [{ patterns: [`${p}.py`, `${p}/*.py`], source: cfg!.source }];
}

/** adr-tools' .adr-dir. */
function adr(inputs: RoleInputs): Part[] | undefined {
  const text = readText(inputs, ".adr-dir");
  const dir = text === undefined ? undefined : cleanPath(text.trim());
  return dir ? [{ patterns: [under(dir, "**")], source: ".adr-dir" }] : undefined;
}

/**
 * Plain-SQL migration tools, each with its own default (Flyway, Supabase, dbmate; sqitch and
 * Atlas only with their config file), and Flyway's filesystem locations from its config.
 */
function sqlMigrations(inputs: RoleInputs, notes: string[]): Part[] {
  const parts: Part[] = [];
  let flyway: Part | undefined;
  for (const f of ["flyway.conf", "conf/flyway.conf"]) {
    const text = readText(inputs, f);
    const m = text === undefined ? null : /^\s*flyway\.locations\s*=\s*(.+)$/m.exec(text);
    if (m) {
      flyway = flywayPart(m[1]!.split(","), f, notes);
      break;
    }
  }
  const toml = readTomlAt(inputs, "flyway.toml")?.flyway?.locations;
  if (!flyway && toml !== undefined) flyway = flywayPart(list(toml), "flyway.toml", notes);
  parts.push(flyway ?? { patterns: ["db/migration/**/*.sql"], source: FORGE_DEFAULT });
  parts.push({ patterns: ["supabase/migrations/*.sql", "db/migrations/*.sql"], source: FORGE_DEFAULT });
  if (existsSync(join(inputs.repo, "sqitch.plan"))) {
    const top = cleanPath(parseIni(readText(inputs, "sqitch.conf") ?? "").get("core")?.get("top_dir") ?? "") ?? "";
    parts.push({ patterns: [under(top, "deploy/**/*.sql")], source: "sqitch.plan" });
  }
  const atlas = readText(inputs, "atlas.hcl");
  if (atlas !== undefined) {
    const dirs = [...atlas.matchAll(/\bdir\s*=\s*"file:\/\/([^"?]+)/g)].map((m) => cleanPath(m[1]!)).filter((d): d is string => d !== undefined);
    parts.push({ patterns: (dirs.length ? [...new Set(dirs)] : ["migrations"]).map((d) => under(d, "*.sql")), source: "atlas.hcl" });
  }
  return parts;
}

function flywayPart(locations: string[], source: string, notes: string[]): Part | undefined {
  const dirs: string[] = [];
  for (const raw of locations.map((l) => l.trim()).filter(Boolean)) {
    const m = /^filesystem:(.+)$/.exec(raw);
    const d = m ? cleanPath(m[1]!) : undefined;
    if (d === undefined) notes.push(`${source}: location ${JSON.stringify(raw)} is not a directory of the repository (set files: sql-migrations in forge.yaml)`);
    else dirs.push(d);
  }
  return dirs.length ? { patterns: dirs.map((d) => under(d, "**/*.sql")), source } : undefined;
}

/**
 * ESM files: .mts and .mjs, and .ts/.tsx/.js/.jsx under a package.json with "type":
 * "module" (Node's rule: the nearest package.json decides), less those under a nested
 * package.json without it.
 */
function esmModules(inputs: RoleInputs): Part[] | undefined {
  const manifests = inputs.files.filter((f) => /(^|\/)package\.json$/.test(f) && !/(^|\/)node_modules\//.test(f)).sort();
  const dirOf = (f: string) => posix.dirname(f).replace(/^\.$/, "");
  const esm = new Set(manifests.filter((f) => readJsonAt(inputs, f)?.type === "module").map(dirOf));
  if (!esm.size) return undefined;
  const ext = "*.{ts,tsx,js,jsx}";
  const patterns = ["**/*.mts", "**/*.mjs"];
  for (const d of esm) patterns.push(under(d, `**/${ext}`));
  // a nested package without "type": "module" under an ESM one is CommonJS again
  for (const f of manifests) {
    const d = dirOf(f);
    if (esm.has(d)) continue;
    if ([...esm].some((e) => e === "" || d.startsWith(`${e}/`))) patterns.push(`!${under(d, `**/${ext}`)}`);
  }
  return [{ patterns, source: "package.json" }];
}

// ---------------------------------------------------------------- the roles

export const ROLES: Record<string, RoleDef> = {
  adr: { about: "architecture decision records", defaults: ["docs/adr/**", "doc/adr/**", "docs/decisions/**"], config: adr },
  "alembic-migrations": { about: "Alembic revision files", defaults: ["alembic/versions/**", "migrations/versions/**"], config: alembicMigrations },
  "better-auth-config": {
    about: "the Better Auth config (the auth CLI's search paths)",
    defaults: ["{,src/,app/}{,lib/server/,server/auth/,server/,auth/,lib/,utils/}{auth,auth/index}{.ts,.tsx,.js,.jsx,.server.js,.server.ts}"],
  },
  compose: { about: "Docker Compose files", defaults: ["**/compose*.yaml", "**/compose*.yml", "**/docker-compose*.yaml", "**/docker-compose*.yml"] },
  "django-migrations": { about: "Django migration modules", defaults: ["**/migrations/[0-9]*.py"] },
  "django-settings": { about: "Django settings modules", defaults: ["**/settings.py", "**/settings/*.py"], config: djangoSettings },
  "django-templates": { about: "Django templates", defaults: ["**/templates/**/*.html", "**/templates/**/*.txt"] },
  docs: { about: "documentation", defaults: ["docs/**/*.{md,mdx,rst}", "doc/**/*.{md,mdx,rst}", "README*"] },
  dockerfile: { about: "Dockerfiles and Containerfiles", defaults: ["**/Dockerfile", "**/Dockerfile.*", "**/*.Dockerfile", "**/Containerfile", "!**/*.dockerignore"] },
  "drizzle-migrations": { about: "Drizzle migrations", defaults: ["drizzle/**"] },
  "e2e-tests": { about: "end-to-end tests", defaults: ["e2e/**", "tests/e2e/**"] },
  "env-files": { about: "dotenv files", defaults: ["**/.env", "**/.env.*", "!@env-template"] },
  "env-template": { about: "dotenv templates", defaults: ["**/.env.example", "**/.env.sample", "**/.env.template", "**/.env.dist"] },
  "esm-modules": { about: "ES module files", defaults: ["**/*.mts", "**/*.mjs"], config: esmModules },
  "generated-clients": { about: "generated API and database clients", defaults: [], config: generatedClients },
  "gunicorn-config": { about: "Gunicorn config files", defaults: ["**/gunicorn.conf.py"] },
  lockfiles: { about: "dependency lock files", defaults: ["uv.lock", "bun.lock", "bun.lockb", "package-lock.json", "pnpm-lock.yaml", "yarn.lock"] },
  "prisma-migrations": { about: "Prisma migrations", defaults: ["prisma/migrations/**"], config: prismaMigrations },
  procfile: { about: "Procfiles", defaults: ["Procfile", "Procfile.*"] },
  "python-tests": { about: "pytest test files", defaults: ["**/test_*.py", "**/*_test.py", "**/conftest.py"], config: pythonTests },
  "sql-migrations": {
    about: "plain SQL migrations (Flyway, Supabase, dbmate; sqitch and Atlas with their config file)",
    defaults: ["db/migration/**/*.sql", "supabase/migrations/*.sql", "db/migrations/*.sql"],
    config: sqlMigrations,
  },
  "ts-tests": { about: "TypeScript and JavaScript test files", defaults: ["**/*.{test,spec}.{ts,tsx,mts,cts,js,jsx,mjs,cjs}", "**/__tests__/**"] },
  "typeorm-datasource": { about: "the TypeORM data source file (no default: set it in forge.yaml)", defaults: [] },
  "wrangler-config": { about: "Wrangler configs", defaults: ["**/wrangler.toml", "**/wrangler.json", "**/wrangler.jsonc"] },
  "wrangler-types": { about: "the `wrangler types` output", defaults: ["worker-configuration.d.ts"] },
};

export function isRole(name: string): boolean {
  return Object.hasOwn(ROLES, name);
}

// ---------------------------------------------------------------- resolution

/**
 * Every role's effective patterns: the defaults (or the tool's config), then forge.yaml.
 * Without inputs (the self-test), Forge's defaults only.
 */
export function resolveRoles(inputs?: RoleInputs, config?: ProjectConfig["files"]): Roles {
  for (const name of Object.keys(config ?? {}))
    if (!isRole(name)) throw new Error(`forge.yaml: files: unknown file role "${name}" (known: ${Object.keys(ROLES).join(", ")})`);
  const raw = new Map<string, { glob: string; source: string }[]>();
  const notes = new Map<string, string[]>();
  for (const [name, def] of Object.entries(ROLES)) {
    const n: string[] = [];
    const cfg = config?.[name];
    let parts: Part[];
    if (cfg?.set) parts = [{ patterns: cfg.set, source: FORGE_YAML }];
    else parts = (inputs && def.config?.(inputs, n)) || [{ patterns: def.defaults, source: FORGE_DEFAULT }];
    if (cfg?.add) parts.push({ patterns: cfg.add, source: FORGE_YAML });
    raw.set(name, parts.flatMap((p) => p.patterns.map((glob) => ({ glob, source: p.source }))));
    notes.set(name, n);
  }
  const done: Roles = new Map();
  const expand = (name: string, stack: string[]): EffectiveRole => {
    const have = done.get(name);
    if (have) return have;
    if (stack.includes(name)) throw new Error(`file roles: @${[...stack, name].join(" -> @")} is a cycle`);
    const patterns: RolePattern[] = [];
    for (const p of raw.get(name)!) {
      const ref = REF.exec(p.glob);
      if (!ref) {
        patterns.push(p.glob.startsWith("!") ? { glob: p.glob.slice(1), exclude: true, source: p.source } : { glob: p.glob, exclude: false, source: p.source });
        continue;
      }
      if (!isRole(ref[2]!)) throw new Error(`file role ${name}: unknown file role "@${ref[2]}"`);
      patterns.push(...negate(expand(ref[2]!, [...stack, name]).patterns, ref[1] === "!", `@${name}`));
    }
    const role = { patterns: dedupe(patterns), notes: notes.get(name)! };
    done.set(name, role);
    return role;
  };
  for (const name of Object.keys(ROLES)) expand(name, []);
  return done;
}

/** Forge's defaults only: what the self-test and rule examples use. */
let defaults: Roles | undefined;
export function defaultRoles(): Roles {
  return (defaults ??= resolveRoles());
}

function negate(patterns: RolePattern[], neg: boolean, where: string): RolePattern[] {
  if (!neg) return patterns;
  if (patterns.some((p) => p.exclude)) throw new Error(`${where}: a role with exclusions cannot be negated`);
  return patterns.map((p) => ({ ...p, exclude: true }));
}

function dedupe(patterns: RolePattern[]): RolePattern[] {
  const seen = new Set<string>();
  return patterns.filter((p) => {
    const k = `${p.exclude ? "!" : ""}${p.glob}`;
    if (seen.has(k)) return false;
    seen.add(k);
    return true;
  });
}

/** A role's patterns as glob-list entries ("!" before an exclusion). */
export function roleGlobs(role: EffectiveRole): string[] {
  return role.patterns.map((p) => (p.exclude ? `!${p.glob}` : p.glob));
}

// ---------------------------------------------------------------- roles in rules

/** The roles a rule names: in scope.files, when_changed, check commands and where modules. */
export function rolesOfRule(rule: Pick<Rule, "scope" | "enforce">): string[] {
  const out = new Set<string>();
  const fromList = (l?: string[]) => l?.forEach((g) => REF.test(g) && out.add(REF.exec(g)![2]!));
  fromList(rule.scope.files);
  for (const t of rule.enforce) {
    if (t.target === "check") {
      fromList(t.when_changed);
      for (const cmd of [t.run, t.changed_run, t.fix_run, t.format_stdin, t.rerun]) for (const m of (cmd ?? "").matchAll(COMMAND_REF)) out.add(m[1]!);
    }
    if (t.target === "ast-grep") for (const c of Object.values(t.where ?? {})) for (const m of whereModules(c)) if (MODULE_REF.test(m)) out.add(MODULE_REF.exec(m)![1]!);
  }
  return [...out].sort();
}

function whereModules(c: WhereCondition): string[] {
  if ("not" in c) return whereModules(c.not);
  const m = c.type.module;
  return m === undefined ? [] : Array.isArray(m) ? m : [m];
}

/** The roles a rule names that Forge does not know. */
export function unknownRoles(rule: Pick<Rule, "scope" | "enforce">): string[] {
  return rolesOfRule(rule).filter((r) => !isRole(r));
}

/** A glob list with its roles expanded; `empty` names the roles that have no file pattern. */
export function expandGlobs(globs: string[], roles: Roles): { globs: string[]; empty: string[] } {
  const out: string[] = [];
  const empty: string[] = [];
  for (const g of globs) {
    const ref = REF.exec(g);
    if (!ref) {
      out.push(g);
      continue;
    }
    const role = roles.get(ref[2]!);
    if (!role) throw new Error(`unknown file role "@${ref[2]}"`);
    if (!role.patterns.some((p) => !p.exclude)) empty.push(ref[2]!);
    out.push(...roleGlobs({ ...role, patterns: negate(role.patterns, ref[1] === "!", g) }));
  }
  return { globs: [...new Set(out)], empty };
}

/**
 * A where module naming a role (`@wrangler-types`): the modules of the role's files, as the
 * types engine names a project file's module (TypeScript: the path without its extension;
 * Python: dotted, without a leading src.). Such a role lists files, not globs.
 */
function roleModules(name: string, roles: Roles, python: boolean): string[] {
  const role = roles.get(name)!;
  return role.patterns
    .filter((p) => !p.exclude)
    .map((p) => {
      if (/[*?[{]/.test(p.glob)) throw new Error(`file role @${name} is a where module: its patterns must be files, not globs (${p.glob})`);
      return python ? p.glob.replace(/\.pyi?$/, "").replace(/\/__init__$/, "").replace(/^src\//, "").replaceAll("/", ".") : p.glob.replace(/(\.d)?\.[cm]?[jt]sx?$/, "");
    });
}

function mapWhere(c: WhereCondition, roles: Roles, python: boolean): WhereCondition {
  if ("not" in c) return { not: mapWhere(c.not, roles, python) };
  const m = c.type.module;
  if (m === undefined) return c;
  const mods = (Array.isArray(m) ? m : [m]).flatMap((x) => (MODULE_REF.test(x) ? roleModules(MODULE_REF.exec(x)![1]!, roles, python) : [x]));
  const type: TypeSpec = { ...c.type, module: mods };
  return { type };
}

/**
 * The rule with its roles replaced by their effective patterns (scope.files, when_changed,
 * where modules), as apply renders it; `empty` lists what leaves it without files: a role
 * with no pattern that was the only file selection of the scope or of a when_changed.
 */
export function applyRoles(rule: Rule, roles: Roles): { rule: Rule; empty: string[] } {
  const empty = new Set<string>();
  const list = (globs: string[] | undefined): string[] | undefined => {
    if (!globs) return undefined;
    const r = expandGlobs(globs, roles);
    if (globs.some((g) => !g.startsWith("!")) && !r.globs.some((g) => !g.startsWith("!"))) for (const e of r.empty) empty.add(e);
    return r.globs;
  };
  if (!rolesOfRule(rule).length) return { rule, empty: [] };
  const files = list(rule.scope.files);
  const python = rule.scope.language === "python";
  const enforce = rule.enforce.map((t) => {
    if (t.target === "check" && t.when_changed) return { ...t, when_changed: list(t.when_changed)! };
    if (t.target === "ast-grep" && t.where) return { ...t, where: Object.fromEntries(Object.entries(t.where).map(([k, c]) => [k, mapWhere(c, roles, python)])) };
    return t;
  });
  return { rule: { ...rule, scope: { ...rule.scope, ...(files ? { files } : {}) }, enforce }, empty: [...empty].sort() };
}
