/**
 * @spec 15.4 · File roles
 *
 * Rules and helpers name the files they are about by **role**, not by literal globs:
 * `scope.files: ["@python-tests"]`, `"!@alembic-migrations"`, `when_changed: ["@compose"]`,
 * `forge_role @<role> <path>` in check commands, `module: "@wrangler-types"` in a where.
 * Each role has Forge's defaults; the tool's own config replaces them when it is data
 * (INI, TOML, JSON, `schema.prisma`, never code); `forge.yaml` adds to or replaces them.
 * `forge apply` renders the effective patterns literally into the generated files, so
 * hooks never read a config; `forge status` shows them, and a changed source is drift.
 */
import { describe, expect, test } from "bun:test";
import { mkdtempSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { loadRuleFile } from "../../src/packs";
import { plan, readProjectInputs } from "../../src/plan";
import { renderRoles } from "../../src/render";
import { applyRoles, defaultRoles, resolveRoles, roleGlobs, type Roles } from "../../src/roles";
import type { Rule } from "../../src/types";
import { commitAll, forge, git, put, ROOT, sampleRepo, sh } from "../helpers";
import { loadPacks } from "../../src/packs";

const T = 120_000;
const PACKS = loadPacks(join(ROOT, "packs"));

/** A directory holding the given files. */
function dir(files: Record<string, string>): string {
  const d = mkdtempSync(join(tmpdir(), "forge-roles-"));
  for (const [rel, text] of Object.entries(files)) put(d, rel, text);
  return d;
}
/** A role's patterns (with "!" before exclusions) for a directory holding these files. */
function role(name: string, files: Record<string, string>, config?: Parameters<typeof resolveRoles>[1]): string[] {
  const d = dir(files);
  return roleGlobs(resolveRoles({ repo: d, files: Object.keys(files) }, config).get(name)!);
}
function source(name: string, files: Record<string, string>): string[] {
  const d = dir(files);
  return [...new Set(resolveRoles({ repo: d, files: Object.keys(files) }).get(name)!.patterns.map((p) => p.source))];
}
function rule(fields: Partial<Rule>): Rule {
  return { id: "local/x", description: "X.", scope: { language: "python" }, enforce: [], file: "x.yaml", hash: "0", origin: "local", pack: "local", ...fields } as Rule;
}
/** forge_role <role> <path> run in sh with the rendered roles: true when it succeeds. */
function forgeRole(roles: Roles, name: string, path: string): boolean {
  return sh("sh", ["-c", `${renderRoles(roles).join("\n")}\nforge_role "$1" "$2"`, "sh", name, path]).status === 0;
}

describe("roles in rules", () => {
  test("@role in scope.files and when_changed becomes the role's patterns, !@role its exclusion, as apply renders the rule", () => {
    const r = applyRoles(
      rule({
        scope: { language: "python", files: ["@django-migrations", "!@python-tests"] },
        enforce: [{ target: "check", stage: "lint", run: "true", when_changed: ["@django-settings"] }],
      }),
      defaultRoles(),
    ).rule;
    expect(r.scope.files).toEqual(["**/migrations/[0-9]*.py", "!**/test_*.py", "!**/*_test.py", "!**/conftest.py"]);
    expect(r.enforce[0]).toMatchObject({ when_changed: ["**/settings.py", "**/settings/*.py"] });
  });

  test("a role may include another, and a role's own exclusions travel with it: @env-files is .env files less the templates", () => {
    expect(roleGlobs(defaultRoles().get("env-files")!)).toEqual(["**/.env", "**/.env.*", "!**/.env.example", "!**/.env.sample", "!**/.env.template", "!**/.env.dist"]);
    const roles = defaultRoles();
    expect(forgeRole(roles, "@env-files", "api/.env.local")).toBe(true);
    expect(forgeRole(roles, "@env-files", ".env.example")).toBe(false);
  });

  test("forge_role <@role> <path> in check helpers answers from the rendered patterns, character classes included", () => {
    const roles = defaultRoles();
    expect(forgeRole(roles, "@django-migrations", "shop/migrations/0002_order.py")).toBe(true);
    expect(forgeRole(roles, "django-migrations", "shop/migrations/0002_order.py")).toBe(true);
    expect(forgeRole(roles, "@django-migrations", "shop/migrations/__init__.py")).toBe(false);
    expect(forgeRole(roles, "@dockerfile", "deploy/api.Dockerfile")).toBe(true);
    expect(forgeRole(roles, "@dockerfile", "Dockerfile.dockerignore")).toBe(false);
    expect(forgeRole(roles, "@no-such-role", "Dockerfile")).toBe(false);
  });

  test("a rule naming an unknown role is rejected when it is loaded", () => {
    const d = dir({ "x.yaml": "id: local/x\ndescription: X.\nscope: {language: python, files: [\"@pytest-files\"]}\nenforce:\n  - {target: grep, mode: forbid, pattern: x}\n" });
    expect(() => loadRuleFile(join(d, "x.yaml"), "local")).toThrow('unknown file role @pytest-files');
  });

  test("a where module may name a role: the role's files are the modules the types engine compares (TypeScript: the path without its extension)", () => {
    const r = applyRoles(
      rule({
        scope: { language: "typescript" },
        enforce: [{ target: "ast-grep", mode: "forbid", rule: { pattern: "$C.x()" }, where: { C: { type: { module: ["@cloudflare/workers-types", "@wrangler-types"], name: "ExecutionContext" } } } }],
      }),
      resolveRoles(undefined, { "wrangler-types": { set: ["apps/api/worker-configuration.d.ts"] } }),
    ).rule;
    expect(r.enforce[0]).toMatchObject({ where: { C: { type: { module: ["@cloudflare/workers-types", "apps/api/worker-configuration"] } } } });
    expect(() =>
      applyRoles(rule({ scope: { language: "typescript" }, enforce: [{ target: "ast-grep", mode: "forbid", rule: { pattern: "$C" }, where: { C: { type: { module: "@ts-tests" } } } }] }), defaultRoles()),
    ).toThrow("must be files, not globs");
  });

  test("a rule whose files are a role with no pattern is not enforced, and says how to set the role; it never runs on other files", () => {
    const repo = sampleRepo("fastapi-app");
    put(repo, ".forge/rules/ds.yaml", "id: python/ds\ndescription: X.\nscope: {language: any, files: [\"@typeorm-datasource\"]}\nenforce:\n  - {target: grep, mode: forbid, pattern: x}\nexamples: {match: [x], no_match: [y]}\n");
    const p = plan({ level: "project", base: repo, packs: PACKS, ...readProjectInputs(repo) });
    const st = p.active.find((a) => a.rule.id === "python/ds")!;
    expect(st.enforced).toEqual([]);
    expect(st.notEnforced).toEqual(["not enforced here: file role @typeorm-datasource has no files (set them in forge.yaml: files: {typeorm-datasource: {set: [...]}})"]);
    expect(p.files.get(".forge/check.sh")).not.toContain("python/ds");
  });
});

describe("defaults from the tool's config", () => {
  test("pytest testpaths × python_files replace the defaults of @python-tests (conftest.py stays: pytest's own name)", () => {
    expect(role("python-tests", { "pyproject.toml": '[tool.pytest.ini_options]\ntestpaths = ["tests", "integration"]\npython_files = ["check_*.py"]\n' })).toEqual([
      "tests/**/check_*.py",
      "integration/**/check_*.py",
      "**/conftest.py",
    ]);
    expect(role("python-tests", { "pytest.ini": "[pytest]\ntestpaths = tests\n" })).toEqual(["tests/**/test_*.py", "tests/**/*_test.py", "**/conftest.py"]);
    expect(source("python-tests", { "pytest.ini": "[pytest]\ntestpaths = tests\n" })).toEqual(["pytest.ini"]);
    expect(role("python-tests", { "pyproject.toml": "[project]\nname = 'x'\n" })).toEqual(["**/test_*.py", "**/*_test.py", "**/conftest.py"]);
  });

  test("alembic.ini script_location (or version_locations) replaces the defaults of @alembic-migrations; a package resource is reported, not guessed", () => {
    expect(role("alembic-migrations", { "alembic.ini": "[alembic]\nscript_location = %(here)s/db/alembic\n" })).toEqual(["db/alembic/versions/**"]);
    expect(role("alembic-migrations", { "alembic.ini": "[alembic]\nscript_location = db\nversion_locations = %(here)s/db/v1 %(here)s/db/v2\n" })).toEqual(["db/v1/**", "db/v2/**"]);
    const d = dir({ "alembic.ini": "[alembic]\nscript_location = myapp:migrations\n" });
    const r = resolveRoles({ repo: d, files: ["alembic.ini"] }).get("alembic-migrations")!;
    expect(roleGlobs(r)).toEqual(["alembic/versions/**", "migrations/versions/**"]);
    expect(r.notes).toEqual(['alembic.ini: script_location "myapp:migrations" is not a path in the repository (set files: alembic-migrations in forge.yaml)']);
  });

  test("schema.prisma generator outputs are @generated-clients; package.json prisma.schema moves @prisma-migrations", () => {
    expect(role("generated-clients", { "prisma/schema.prisma": 'generator client {\n  provider = "prisma-client"\n  output   = "../src/generated/prisma"\n}\n' })).toEqual(["src/generated/prisma/**"]);
    expect(role("generated-clients", { "prisma/schema.prisma": 'generator client {\n  provider = "prisma-client-js"\n}\n' })).toEqual([]);
    expect(role("prisma-migrations", { "package.json": '{"prisma": {"schema": "db/schema.prisma"}}' })).toEqual(["db/migrations/**"]);
  });

  test("DJANGO_SETTINGS_MODULE of pytest-django names @django-settings; .adr-dir names @adr", () => {
    expect(role("django-settings", { "pyproject.toml": '[tool.pytest.ini_options]\nDJANGO_SETTINGS_MODULE = "mysite.settings.test"\n' })).toEqual([
      "mysite/settings/test.py",
      "mysite/settings/test/*.py",
    ]);
    expect(role("adr", { ".adr-dir": "doc/architecture/decisions\n" })).toEqual(["doc/architecture/decisions/**"]);
  });

  test("@sql-migrations has each tool's default; Flyway's filesystem locations replace Flyway's, and sqitch and Atlas count only with their config file", () => {
    expect(role("sql-migrations", {})).toEqual(["db/migration/**/*.sql", "supabase/migrations/*.sql", "db/migrations/*.sql"]);
    expect(role("sql-migrations", { "flyway.conf": "flyway.locations=filesystem:sql\n", "sqitch.plan": "%project=x\n", "atlas.hcl": 'env "dev" {\n  migration {\n    dir = "file://db/atlas"\n  }\n}\n' })).toEqual([
      "sql/**/*.sql",
      "supabase/migrations/*.sql",
      "db/migrations/*.sql",
      "deploy/**/*.sql",
      "db/atlas/*.sql",
    ]);
  });

  test("@esm-modules: .mts and .mjs, and the .ts/.js files under a package.json with type: module, less a nested CommonJS package", () => {
    const files = { "package.json": '{"type": "module"}', "legacy/package.json": "{}", "src/a.ts": "" };
    expect(role("esm-modules", files)).toEqual(["**/*.mts", "**/*.mjs", "**/*.{ts,tsx,js,jsx}", "!legacy/**/*.{ts,tsx,js,jsx}"]);
    const d = dir(files);
    const roles = resolveRoles({ repo: d, files: Object.keys(files) });
    expect(forgeRole(roles, "@esm-modules", "src/a.ts")).toBe(true);
    expect(forgeRole(roles, "@esm-modules", "legacy/b.js")).toBe(false);
    expect(forgeRole(roles, "@esm-modules", "legacy/b.mjs")).toBe(true);
    expect(role("esm-modules", { "package.json": "{}" })).toEqual(["**/*.mts", "**/*.mjs"]);
  });

  test("a code config is never evaluated: a vitest.config.ts leaves @ts-tests at Forge's defaults", () => {
    expect(role("ts-tests", { "vitest.config.ts": "export default { test: { include: ['spec/**/*.ts'] } }\n" })).toEqual(roleGlobs(defaultRoles().get("ts-tests")!));
  });
});

describe("forge.yaml", () => {
  test("files: <role>: add adds patterns to a role, set replaces its defaults and the tool's config", () => {
    expect(role("python-tests", { "pytest.ini": "[pytest]\ntestpaths = tests\n" }, { "python-tests": { add: ["integration/**/check_*.py"] } })).toEqual([
      "tests/**/test_*.py",
      "tests/**/*_test.py",
      "**/conftest.py",
      "integration/**/check_*.py",
    ]);
    expect(role("env-files", {}, { "env-files": { set: [".env", ".env.local"] } })).toEqual([".env", ".env.local"]);
  });

  test("an unknown role and a cycle between roles are rejected", () => {
    expect(() => resolveRoles(undefined, { "pytest-files": { add: ["x"] } })).toThrow('forge.yaml: files: unknown file role "pytest-files"');
    expect(() => resolveRoles(undefined, { "env-template": { set: ["@env-files"] } })).toThrow("is a cycle");
    const repo = sampleRepo("fastapi-app");
    put(repo, "forge.yaml", 'forge: "0.5"\nfiles:\n  python-tests: {replace: ["x"]}\n');
    const r = forge(["status"], repo);
    expect(r.status).not.toBe(0);
    expect(r.stderr).toContain("forge.yaml: invalid project");
  });
});

describe("status and drift", () => {
  test(
    "forge status prints a Files section: each role an active rule names, its patterns and where each comes from",
    () => {
      const repo = sampleRepo("fastapi-app");
      put(repo, "forge.yaml", 'forge: "0.5"\nfiles:\n  python-tests: {add: ["integration/**/check_*.py"]}\n');
      const r = forge(["status"], repo);
      expect(r.status).toBe(0);
      expect(r.stdout).toContain("\nFiles:\n");
      expect(r.stdout).toContain("  @alembic-migrations: alembic/versions/** (alembic.ini)\n");
      expect(r.stdout).toContain("  @python-tests: **/test_*.py, **/*_test.py, **/conftest.py (forge default); integration/**/check_*.py (forge.yaml)\n");
      const json = JSON.parse(forge(["status", "--json"], repo).stdout);
      expect(json.files.find((f: { name: string }) => f.name === "alembic-migrations")).toMatchObject({ patterns: ["alembic/versions/**"], sources: ["alembic.ini"] });
    },
    T,
  );

  test(
    "a role an active rule names but no file matches is reported by status",
    () => {
      const repo = sampleRepo("fastapi-app");
      put(repo, "alembic.ini", "[alembic]\nscript_location = db\n");
      expect(forge(["status"], repo).stdout).toContain("  @alembic-migrations: db/versions/** (alembic.ini); matches no file here\n");
    },
    T,
  );

  test(
    "a changed source of a role is drift: status and diff report it, and apply renders the new patterns",
    () => {
      const repo = sampleRepo("fastapi-app");
      expect(forge(["apply", "--yes"], repo).status).toBe(0);
      commitAll(repo);
      expect(readFileSync(join(repo, ".forge/forge.lock.json"), "utf8")).toContain('"alembic-migrations": [\n      "alembic/versions/**"\n    ]');
      put(repo, "alembic.ini", "[alembic]\nscript_location = db\n");
      expect(forge(["status"], repo).stdout).toContain("  files @alembic-migrations changed since the last apply: db/versions/** (alembic.ini)\n");
      const diff = forge(["diff"], repo);
      expect(diff.status).toBe(1);
      expect(diff.stdout).toContain("forge: files @alembic-migrations changed since the last apply: db/versions/** (alembic.ini)");
      expect(forge(["apply", "--yes"], repo).status).toBe(0);
      expect(readFileSync(join(repo, ".forge/check.sh"), "utf8")).toContain("    alembic-migrations) case $2 in db/versions/*) return 0 ;; esac ;;");
      expect(forge(["status"], repo).stdout).not.toContain("changed since the last apply");
    },
    T,
  );
});

describe("rules on roles", () => {
  /** A TypeScript project with TypeORM, its typeorm-ts-node-commonjs replaced by a script printing <out> (exit <code>). */
  function typeormRepo(out: string, code: number, forgeYaml = ""): string {
    const repo = mkdtempSync(join(tmpdir(), "forge-typeorm-"));
    git(repo, "init", "-q", "-b", "main");
    put(repo, "package.json", '{"name": "api", "private": true, "dependencies": {"typeorm": "1.1.2"}}\n');
    put(repo, "tsconfig.json", '{"compilerOptions": {"strict": true}}\n');
    put(repo, ".gitignore", "node_modules/\n");
    put(repo, "src/data-source.ts", 'import { DataSource } from "typeorm";\n\nexport default new DataSource({ type: "postgres", url: process.env.DATABASE_URL });\n');
    put(repo, "src/user.entity.ts", "export class User {\n  id!: number;\n}\n");
    put(repo, "node_modules/.bin/typeorm-ts-node-commonjs", `#!/bin/sh\necho "$*" >> calls.log\ncase $1 in migration:run) exit 0 ;; esac\necho '${out}'\nexit ${code}\n`);
    sh("chmod", ["+x", join(repo, "node_modules/.bin/typeorm-ts-node-commonjs")]);
    if (forgeYaml) put(repo, "forge.yaml", forgeYaml);
    expect(forge(["apply", "--yes"], repo).status).toBe(0);
    commitAll(repo, "init");
    put(repo, "src/user.entity.ts", "export class User {\n  id!: number;\n  name!: string;\n}\n");
    return repo;
  }
  const DS = 'forge: "0.5"\nfiles:\n  typeorm-datasource: {set: [src/data-source.ts]}\n';
  const run = (repo: string) => sh("sh", [".forge/check.sh", "--changed"], { cwd: repo, env: { DATABASE_URL: "postgres://app@localhost:5432/app" } });

  /** @rule typescript/typeorm/migration-present */
  test(
    "the data source is file role @typeorm-datasource: the migrations run, then migration:generate --check fails on a schema change no migration has",
    () => {
      const repo = typeormRepo("Unexpected changes in database schema were found in check mode:", 1, DS);
      const r = run(repo);
      expect(r.status).toBe(1);
      expect(r.stdout).toContain("forge:typescript/typeorm/migration-present failed");
      expect(r.stdout).toContain("generate one (typeorm-ts-node-commonjs migration:generate -d src/data-source.ts <path>)");
      expect(readFileSync(join(repo, "calls.log"), "utf8").split("\n")[0]).toBe("migration:run -d src/data-source.ts");
      expect(run(typeormRepo("No changes in database schema were found", 0, DS)).status).toBe(0);
    },
    T,
  );

  /** @rule typescript/typeorm/migration-present */
  test(
    "without a data source in forge.yaml, the TypeORM check reports itself skipped and runs nothing",
    () => {
      const repo = typeormRepo("Unexpected changes in database schema were found in check mode:", 1);
      const r = run(repo);
      expect(r.status).toBe(0);
      expect(r.stdout).toContain("forge:typescript/typeorm/migration-present skipped: file role @typeorm-datasource names no file");
      expect(forge(["status"], repo).stdout).toContain("  @typeorm-datasource: no patterns; matches no file here\n");
    },
    T,
  );
});
