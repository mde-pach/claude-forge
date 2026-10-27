/**
 * @spec 5 · Packs and project inputs
 *
 * Packs form a tree: **language first, then package, then sub-package**. A root
 * pack is active when detected; a child is active when its parent is active and
 * it is detected itself, so the same package can carry different rules under
 * different parents (`typescript/nextjs/react`, `typescript/astro/react`).
 * Detection reads `pyproject.toml`, `uv.lock`, `package.json` and `bun.lock`.
 * `forge.yaml` and `.forge/rules/` adjust the result; both are inputs Forge
 * never rewrites.
 */
import { describe, expect, test } from "bun:test";
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { detectAnywhere, lineage, loadPacks, readProjectFacts, workspaceDirs } from "../../src/packs";
import { plan, readProjectInputs } from "../../src/plan";
import { renderLib } from "../../src/render";
import { auditRules } from "../../src/audit";
import { parseStrictYaml } from "../../src/yaml";
import { applied, forge, put, ROOT, sampleRepo, sh } from "../helpers";
import type { Pack } from "../../src/types";

const T = 120_000;
const PACKS = loadPacks(join(ROOT, "packs"));

/** Active pack ids for a sample repository, with optional forge.yaml. */
function activePacks(name: string, forgeYaml?: string): string[] {
  const repo = sampleRepo(name);
  if (forgeYaml) put(repo, "forge.yaml", forgeYaml);
  return plan({ level: "project", base: repo, packs: PACKS, ...readProjectInputs(repo) }).packs.map((p) => p.id);
}

describe("the pack tree", () => {
  test("pack ids are paths: language first, then package, then sub-package", () => {
    expect(lineage(PACKS, "python/sqlalchemy/alembic")).toEqual(["python", "python/sqlalchemy", "python/sqlalchemy/alembic"]);
    expect(lineage(PACKS, "typescript/nextjs/react")).toEqual(["typescript", "typescript/nextjs", "typescript/nextjs/react"]);
    expect(lineage(PACKS, "typescript/astro/react")).toEqual(["typescript", "typescript/astro", "typescript/astro/react"]);
    expect(PACKS.get("typescript")!.children).toContain("typescript/nextjs");
  });

  test("rule ids are the pack id plus the rule file name", () => {
    for (const p of PACKS.values()) for (const r of p.rules) expect(r.id).toBe(`${p.id}/${r.file.split("/").pop()!.replace(/\.yaml$/, "")}`);
  });

  /** @pack typescript/nextjs/react typescript/astro/react typescript/react */
  test("a child pack is active only under its active parent: react is under Next.js in a Next.js app and under Astro in an Astro app, while typescript/react applies to both", () => {
    const next = activePacks("nextjs-prisma-app");
    expect(next).toContain("typescript/nextjs/react");
    expect(next).toContain("typescript/react");
    expect(next).not.toContain("typescript/astro/react");
    const astro = activePacks("astro-app");
    expect(astro).toEqual(["git", "secrets", "supply-chain", "typescript", "typescript/astro", "typescript/astro/react", "typescript/bun", "typescript/react"]);
  });

  test("detection follows dependencies with versions, and lock files refine them", () => {
    expect(activePacks("fastapi-app")).toEqual([
      "git",
      "python",
      "python/asyncpg",
      "python/fastapi",
      "python/pydantic",
      "python/pytest",
      "python/sqlalchemy",
      "python/sqlalchemy/alembic",
      "python/uv",
      "secrets",
      "supply-chain",
    ]);
  });

  describe("a broken pack tree is rejected", () => {
    function tree(files: Record<string, string>): () => unknown {
      const dir = mkdtempSync(join(tmpdir(), "forge-tree-"));
      for (const [rel, text] of Object.entries(files)) {
        mkdirSync(join(dir, rel, ".."), { recursive: true });
        writeFileSync(join(dir, rel), text);
      }
      return () => loadPacks(dir);
    }
    test("a pack whose id is not its path", () => {
      expect(tree({ "ts/pack.yaml": "id: ts\ndescription: T.\nlevel: project\n", "ts/x/pack.yaml": "id: x\ndescription: X.\nlevel: project\n" })).toThrow(
        'id "x" must equal its path in the pack tree, "ts/x"',
      );
    });
    test("a child whose level differs from its parent's", () => {
      expect(tree({ "ts/pack.yaml": "id: ts\ndescription: T.\nlevel: project\n", "ts/x/pack.yaml": "id: ts/x\ndescription: X.\nlevel: user\n" })).toThrow(
        "level user differs from parent ts (project)",
      );
    });
    test("a directory inside a pack that is neither rules/, init/ nor a pack", () => {
      expect(tree({ "ts/pack.yaml": "id: ts\ndescription: T.\nlevel: project\n", "ts/stray/file.txt": "x" })).toThrow("not a pack (no pack.yaml)");
    });
  });
});

describe("forge.yaml", () => {
  test("packs.add activates a pack and its ancestors", () => {
    expect(activePacks("fastapi-app", 'forge: "0.5"\npacks: {add: [typescript/astro/react]}\n')).toEqual(
      expect.arrayContaining(["typescript", "typescript/astro", "typescript/astro/react"]),
    );
  });

  test("packs.remove deactivates a pack and its whole subtree", () => {
    const active = activePacks("nextjs-prisma-app", 'forge: "0.5"\npacks: {remove: [typescript/nextjs]}\n');
    expect(active).not.toContain("typescript/nextjs");
    expect(active).not.toContain("typescript/nextjs/react");
    expect(active).toContain("typescript/prisma");
  });

  test(
    "status explains every inactive pack and every disabled rule with its reason",
    () => {
      const repo = sampleRepo("nextjs-prisma-app");
      put(repo, "forge.yaml", 'forge: "0.5"\npacks: {remove: [typescript/nextjs]}\ndisable:\n  - id: typescript/vitest/run\n    reason: slow suite, CI only\n');
      expect(forge(["apply", "--yes"], repo).status).toBe(0);
      const out = forge(["status"], repo).stdout;
      expect(out).toContain("typescript/nextjs: not active (removed in forge.yaml)");
      expect(out).toContain("typescript/nextjs/react: not active (parent typescript/nextjs is not active)");
      expect(out).toContain("typescript/vitest/run: slow suite, CI only");
      expect(readFileSync(join(repo, ".forge/check.sh"), "utf8")).not.toContain("vitest run");
    },
    T,
  );

  test(
    "unknown packs, user-level packs and disables of inactive rules are warnings, not errors",
    () => {
      const repo = sampleRepo("eslint-app");
      put(repo, "forge.yaml", 'forge: "0.5"\npacks: {add: [nosuch, safety]}\ndisable:\n  - {id: python/pytest/run, reason: not here}\n');
      const r = forge(["apply", "--yes"], repo);
      expect(r.status).toBe(0);
      expect(r.stderr).toContain('unknown pack "nosuch"');
      expect(r.stderr).toContain('"safety" is a user-level pack');
      expect(r.stderr).toContain('rule "python/pytest/run" is not active here');
    },
    T,
  );

  test.each([
    ["an unknown key", 'forge: "0.5"\nextra: 1\n', /invalid project.*extra/],
    ["a disable without a reason", 'forge: "0.5"\ndisable:\n  - id: python/pytest/run\n', /reason/],
    ["an accepted config change without a reason", 'forge: "0.5"\naccept:\n  - {rule: python/config-ratchet, change: "pyproject.toml: tool.ruff.lint.ignore adds E501"}\n', /reason/],
    ["a duplicate key", 'forge: "0.5"\nforge: "0.5"\n', /forge\.yaml/],
    ["an alias", 'forge: &v "0.5"\npacks: {add: [*v]}\n', /alias|anchor/],
    ["no version", "packs: {add: [typescript/zod]}\n", /forge/],
  ])(
    "a forge.yaml with %s stops apply and writes nothing",
    (_kind, text, message) => {
      const repo = sampleRepo("fastapi-app");
      put(repo, "forge.yaml", text);
      const r = forge(["apply", "--yes"], repo);
      expect(r.status).toBe(1);
      expect(r.stderr).toMatch(message);
      expect(r.stderr).toContain("forge.yaml");
    },
    T,
  );
});

describe("project-local rules", () => {
  const stricter = `id: typescript/no-ts-ignore
description: No @ts-ignore or @ts-nocheck.
scope: {language: typescript}
enforce:
  - target: ast-grep
    mode: forbid
    rule: {kind: comment, regex: "^//[[:space:]]*@ts-(ignore|nocheck)"}
examples:
  match: ["// @ts-nocheck\\nconst a = 1;\\n"]
  no_match: ["// ok\\nconst a = 1;\\n"]
`;

  test(
    "a local rule with a pack rule's id replaces it entirely",
    () => {
      const repo = sampleRepo("eslint-app");
      put(repo, ".forge/rules/no-ts-ignore.yaml", stricter);
      expect(forge(["apply", "--yes"], repo).status).toBe(0);
      expect(forge(["status"], repo).stdout).toContain("typescript/no-ts-ignore (local)");
      expect(readFileSync(join(repo, ".forge/ast-grep/rules/typescript/no-ts-ignore.yml"), "utf8")).toContain("@ts-(ignore|nocheck)");
    },
    T,
  );

  test.each([
    ["an unknown target", "id: local/x\ndescription: x\nscope: {language: any}\nenforce: [{target: shell, run: x}]\n", /\.forge\/rules\/x\.yaml/],
    ["a malformed id", "id: NotAnId\ndescription: x\nscope: {language: any}\nenforce: [{target: guard, decision: deny, pattern: x}]\n", /id/],
  ])(
    "a local rule with %s stops apply and names the file",
    (_kind, text, message) => {
      const repo = sampleRepo("fastapi-app");
      put(repo, ".forge/rules/x.yaml", text);
      const r = forge(["apply", "--yes"], repo);
      expect(r.status).toBe(1);
      expect(r.stderr).toMatch(message);
    },
    T,
  );

  /**
   * `forge test` checks the project's own rules: those of `.forge/rules` (a local
   * rule replacing a pack rule is tested as written). The bundled packs are proved
   * by Forge's own test suite (`bun run test:packs`), so they are not run here.
   */
  describe("forge test", () => {
    const guard = (match: string) =>
      `id: local/no-force-push\ndescription: No force push.\nscope: {language: any}\nenforce: [{target: guard, decision: deny, pattern: "git push.*--force"}]\nexamples: {match: [${JSON.stringify(match)}], no_match: ["git push"]}\n`;

    test(
      "runs the examples of the local rules only, and passes when they behave as declared",
      () => {
        const repo = sampleRepo("eslint-app");
        put(repo, ".forge/rules/no-force-push.yaml", guard("git push --force"));
        const r = forge(["test"], repo);
        expect(r.status).toBe(0);
        expect(r.stdout).toContain("forge: 1 local rule(s); ast-grep 0, guard 1, permission 0, grep 0, check 0, context 0: all passed");
        expect(r.stdout).not.toContain("packs");
      },
      T,
    );

    test(
      "fails, naming the rule, when a local rule's example does not behave as declared",
      () => {
        const repo = sampleRepo("eslint-app");
        put(repo, ".forge/rules/no-force-push.yaml", guard("git push -f"));
        const r = forge(["test"], repo);
        expect(r.status).toBe(1);
        expect(r.stderr).toContain('forge:test local/no-force-push [sh]: expected deny for "git push -f", got nothing');
      },
      T,
    );

    test(
      "a local rule replacing a pack rule is tested as written",
      () => {
        const repo = sampleRepo("eslint-app");
        put(repo, ".forge/rules/no-ts-ignore.yaml", stricter.replace("// @ts-nocheck", "// fine"));
        const r = forge(["test"], repo);
        expect(r.status).toBe(1);
        expect(r.stderr).toContain("typescript/no-ts-ignore");
      },
      T,
    );

    test(
      "without local rules there is nothing to test; outside a repository it is an error",
      () => {
        const repo = sampleRepo("eslint-app");
        const r = forge(["test"], repo);
        expect(r.status).toBe(0);
        expect(r.stdout).toBe("forge: no local rules in .forge/rules; nothing to test\n");
        const outside = forge(["test"], mkdtempSync(join(tmpdir(), "forge-norepo-")));
        expect(outside.status).toBe(1);
        expect(outside.stderr).toContain("not inside a git repository");
      },
      T,
    );
  });

  test(
    "forge.yaml and .forge/rules are inputs: apply never rewrites them, and Claude must ask to edit them",
    () => {
      const repo = sampleRepo("eslint-app");
      put(repo, "forge.yaml", 'forge: "0.5"\n');
      put(repo, ".forge/rules/no-ts-ignore.yaml", stricter);
      expect(forge(["apply", "--yes"], repo).status).toBe(0);
      expect(readFileSync(join(repo, "forge.yaml"), "utf8")).toBe('forge: "0.5"\n');
      expect(readFileSync(join(repo, ".forge/rules/no-ts-ignore.yaml"), "utf8")).toBe(stricter);
      const settings = JSON.parse(readFileSync(join(repo, ".claude/settings.json"), "utf8"));
      expect(settings.permissions.ask).toEqual(expect.arrayContaining(["Edit(forge.yaml)", "Edit(.forge/**)"]));
    },
    T,
  );
});

/**
 * In a monorepo the project's config often lives in workspace packages
 * (`packages/app/tsconfig.json`, its own `package.json`), not at the root.
 * Detection also looks inside the packages a workspace declares, and checks run
 * where their tool is configured.
 */
describe("workspaces", () => {
  /** A repository from files, its plan. */
  function planOf(files: Record<string, string>) {
    const repo = mkdtempSync(join(tmpdir(), "forge-ws-"));
    for (const [rel, text] of Object.entries(files)) put(repo, rel, text);
    sh("git", ["init", "-q"], { cwd: repo });
    return plan({ level: "project", base: repo, packs: PACKS, ...readProjectInputs(repo) });
  }
  const TS = { "tsconfig.json": "{}\n", "package.json": '{"name": "app", "devDependencies": {"vitest": "4.0.8"}}\n' };
  const inDir = (dir: string, files: Record<string, string>) => Object.fromEntries(Object.entries(files).map(([k, v]) => [`${dir}/${k}`, v]));

  test("packs are detected inside the packages of pnpm-workspace.yaml, with their dependencies", () => {
    const p = planOf({ "package.json": '{"name": "mono", "private": true}\n', "pnpm-workspace.yaml": "packages:\n  - 'libs/*'\n", ...inDir("libs/core", TS) });
    expect(p.packs.map((x) => x.id)).toEqual(["git", "secrets", "supply-chain", "typescript", "typescript/pnpm", "typescript/vitest"]);
  });

  test("package.json workspaces (npm, Yarn, Bun) and uv workspace members declare packages too; excluded ones are not read", () => {
    const npm = planOf({ "package.json": '{"name": "mono", "workspaces": ["apps/*"]}\n', ...inDir("apps/web", TS), ...inDir("other/x", { "package.json": '{"dependencies": {"zod": "4.0.0"}}\n' }) });
    expect(npm.packs.map((x) => x.id)).toEqual(["git", "secrets", "supply-chain", "typescript", "typescript/vitest"]);
    const yarn = planOf({ "package.json": '{"name": "mono", "workspaces": {"packages": ["apps/*"]}}\n', ...inDir("apps/web", TS) });
    expect(yarn.packs.map((x) => x.id)).toContain("typescript");
    const uv = planOf({
      "pyproject.toml": '[project]\nname = "mono"\n\n[tool.uv.workspace]\nmembers = ["services/*"]\nexclude = ["services/old"]\n',
      "services/api/pyproject.toml": '[project]\nname = "api"\ndependencies = ["fastapi>=0.115"]\n',
      "services/old/pyproject.toml": '[project]\nname = "old"\ndependencies = ["pytest>=8"]\n',
    });
    expect(uv.packs.map((x) => x.id)).toEqual(["git", "python", "python/fastapi", "python/pydantic", "secrets", "supply-chain"]);
  });

  test("each package is detected on its own dependencies: next@14 at the root and next@16 in a package activates typescript/nextjs (>=16)", () => {
    const p = planOf({
      "package.json": '{"name": "mono", "private": true, "workspaces": ["apps/*"], "dependencies": {"next": "14.2.0"}}\n',
      "tsconfig.json": "{}\n",
      "apps/old/package.json": '{"name": "old", "dependencies": {"next": "14.2.0"}}\n',
      "apps/new/package.json": '{"name": "new", "dependencies": {"next": "16.0.0"}}\n',
    });
    expect(p.packs.map((x) => x.id)).toContain("typescript/nextjs");
  });

  /**
   * A pack detected only in some packages scopes its ast-grep rule files and grep
   * rules to those packages (as its checks run there); its guard, permission and
   * context entries still apply to the whole repository.
   */
  test("a pack detected only in some packages scopes its ast-grep and grep rules to them", () => {
    const p = planOf({
      "pyproject.toml": '[project]\nname = "mono"\n\n[tool.uv.workspace]\nmembers = ["services/*"]\n',
      "services/api/pyproject.toml": '[project]\nname = "api"\ndependencies = ["fastapi>=0.115"]\n',
      "services/worker/pyproject.toml": '[project]\nname = "worker"\ndependencies = []\n',
    });
    const rule = parseStrictYaml(p.files.get(".forge/ast-grep/rules/python/fastapi/cors-credentials-wildcard.yml")!, "rule") as { files?: string[] };
    expect(rule.files).toEqual(["services/api/**"]);
    // python itself is detected at the root: its rules stay repository-wide
    expect((parseStrictYaml(p.files.get(".forge/ast-grep/rules/python/no-stub.yml")!, "rule") as { files?: string[] }).files).toBeUndefined();
    const vue = planOf({
      "package.json": '{"name": "mono", "private": true, "workspaces": ["packages/*"]}\n',
      "packages/web/package.json": '{"name": "web", "dependencies": {"vue": "^3.4.0"}, "devDependencies": {"typescript": "^5.4.0"}}\n',
      "packages/lib/package.json": '{"name": "lib", "devDependencies": {"typescript": "^5.4.0"}}\n',
    });
    expect(vue.active.find((s) => s.rule.id === "typescript/vue/no-v-html")!.enforced).toEqual(["grep forbid (.forge/check.sh) (in packages/web)"]);
  });

  test("lerna.json declares packages (packages/* by default); without any declaration the repository has no workspace packages", () => {
    expect(planOf({ "lerna.json": "{}\n", ...inDir("packages/lib", TS) }).packs.map((x) => x.id)).toEqual(["git", "secrets", "supply-chain", "typescript", "typescript/vitest"]);
    expect(planOf({ "lerna.json": '{"packages": ["modules/*"]}\n', ...inDir("modules/lib", TS) }).packs.map((x) => x.id)).toEqual(["git", "secrets", "supply-chain", "typescript", "typescript/vitest"]);
    expect(planOf({ "pyproject.toml": '[project]\nname = "app"\n', ...inDir("packages/lib", TS) }).packs.map((x) => x.id)).toEqual(["git", "python", "secrets", "supply-chain"]);
  });

  test("an exclusion like !**/test/** also excludes a package directory named test", () => {
    const files = ["packages/test/package.json", "packages/core/package.json"];
    const repo = mkdtempSync(join(tmpdir(), "forge-ws-"));
    put(repo, "pnpm-workspace.yaml", "packages: ['packages/*', '!**/test/**']\n");
    expect(workspaceDirs(repo, files)).toEqual(["packages/core"]);
  });

  /** 1,500 packages of 200 files: detection reads the file list once, not once per package. */
  test("detection on a large monorepo takes well under a few seconds", () => {
    const files = ["package.json"];
    for (let i = 1; i <= 1500; i++) {
      files.push(`packages/p${i}/package.json`);
      for (let j = 0; j < 200; j++) files.push(`packages/p${i}/src/f${j}.ts`);
    }
    const repo = mkdtempSync(join(tmpdir(), "forge-big-"));
    put(repo, "package.json", '{"name": "r", "private": true, "workspaces": ["packages/*"]}\n');
    const t = performance.now();
    const facts = readProjectFacts(repo, () => files);
    for (const p of PACKS.values()) detectAnywhere(p, facts);
    expect(facts.workspaces.length).toBe(1500);
    expect(performance.now() - t).toBeLessThan(3000);
  });

  /** @rule typescript/vue/typecheck */
  test("the Vue type check (vue-tsc through forge_exec_tool) runs in the package that has Vue", () => {
    const ws = planOf({
      "package.json": '{"name": "mono", "private": true, "workspaces": ["packages/*"]}\n',
      "packages/web/package.json": '{"name": "web", "private": true, "dependencies": {"vue": "^3.4.0"}, "devDependencies": {"vue-tsc": "^2.0.0", "typescript": "^5.4.0"}}\n',
      "packages/web/tsconfig.json": "{}\n",
      "packages/lib/package.json": '{"name": "lib", "private": true, "devDependencies": {"typescript": "^5.4.0"}}\n',
      "packages/lib/tsconfig.json": "{}\n",
    });
    expect(ws.active.find((s) => s.rule.id === "typescript/vue/typecheck")!.enforced).toEqual([expect.stringMatching(/^check typecheck: .*\(in packages\/web\)$/s)]);
  });

  /**
   * Added lines are found against the base copy formatted like the file (§10.1):
   * that formatter runs in the package holding the file, with its config, as the
   * format check itself does.
   * @rule typescript/format-lint
   */
  test("a format_stdin filter runs in the package that configures the formatter, on the path relative to it", () => {
    const biome = "#!/bin/sh\ncat\nprintf '// in %s as %s\\n' \"$(basename \"$PWD\")\" \"$2\"\n";
    const ws = planOf({
      "package.json": '{"name": "mono", "private": true, "workspaces": ["packages/*"]}\n',
      ...inDir("packages/a", { "package.json": '{"name": "a", "devDependencies": {"@biomejs/biome": "2.5.15"}}\n', "biome.json": "{}\n", "tsconfig.json": "{}\n" }),
    });
    put(ws.base, "packages/a/node_modules/.bin/biome", biome);
    chmodSync(join(ws.base, "packages/a/node_modules/.bin/biome"), 0o755);
    put(ws.base, "in.ts", "const a = 1;\n");
    const script = `${renderLib()}\nIFS='\n'\n${ws.checksFragment}\nforge_format_copy packages/a/src/x.ts in.ts out.ts; echo "rc=$?"; cat out.ts`;
    const r = sh("sh", ["-c", script], { cwd: ws.base });
    expect(r.stdout).toBe("rc=0\nconst a = 1;\n// in a as --stdin-file-path=src/x.ts\n");
  });

  /** Each package has its own tool config: each is audited (§11), not only the first. */
  test("a tool configured in several packages is audited in each of them", () => {
    const ws = planOf({ "package.json": '{"name": "mono"}\n', "pnpm-workspace.yaml": "packages: [packages/*]\n", ...inDir("packages/a", TS), ...inDir("packages/b", TS) });
    expect(ws.toolDirs.tsc).toEqual(["packages/a", "packages/b"]);
    const st = ws.active.filter((s) => s.audits.some((t) => t.tool === "tsc")).slice(0, 1);
    expect(auditRules(ws.base, st, (t) => ws.toolDirs[t]).map((a) => a.dir)).toEqual(["packages/a", "packages/b"]);
  });

  test("a check whose tool is configured in a package runs there; at the root, as before, when the root configures it", () => {
    const ws = planOf({ "package.json": '{"name": "mono"}\n', "pnpm-workspace.yaml": "packages: [packages/*]\n", ...inDir("packages/a", TS), ...inDir("packages/b", TS) });
    const typecheck = ws.active.find((s) => s.rule.id === "typescript/typecheck")!;
    const run = (typecheck.rule.enforce[0] as { run: string }).run;
    expect(typecheck.enforced).toEqual([`check typecheck: ${run} (in packages/a, packages/b)`]);
    expect(ws.files.get(".forge/check.sh")).toContain("c_dirs='packages/a packages/b'");
    const root = planOf({ ...TS, "pnpm-workspace.yaml": "packages: [packages/*]\n", ...inDir("packages/a", TS) });
    expect(root.active.find((s) => s.rule.id === "typescript/typecheck")!.enforced).toEqual([`check typecheck: ${run}`]);
  });
});

/**
 * A rule can apply only to some versions of a library (`when: {deps: […]}`): an API
 * removed in Express 5, a setting removed in pnpm 11. It is active only where every
 * listed dependency is declared in its range, judged like detection (lock files refine
 * versions; `package.json` `packageManager` declares the package manager), and in a
 * monorepo only in the packages where the gate holds. A rule left out by its gate is
 * listed by `forge status` with what it needs.
 */
describe("version gates", () => {
  const GATED = `id: local/v5-only
description: Only in Express 5.
scope: {language: typescript}
when: {deps: [{ecosystem: npm, name: express, version: ">=5"}]}
enforce:
  - target: ast-grep
    mode: forbid
    rule: {pattern: "legacy()"}
examples:
  match: ["legacy()"]
  no_match: ["modern()"]
`;
  function planWith(files: Record<string, string>) {
    const repo = mkdtempSync(join(tmpdir(), "forge-gate-"));
    for (const [rel, text] of Object.entries({ "tsconfig.json": "{}\n", ".forge/rules/v5-only.yaml": GATED, ...files })) put(repo, rel, text);
    sh("git", ["init", "-q"], { cwd: repo });
    return { repo, p: plan({ level: "project", base: repo, packs: PACKS, ...readProjectInputs(repo) }) };
  }
  const active = (p: ReturnType<typeof plan>) => p.active.some((s) => s.rule.id === "local/v5-only");

  test("a gated rule is active only when the dependency is declared in its range, and status says what an inactive one needs", () => {
    expect(active(planWith({ "package.json": '{"dependencies": {"express": "^5.1.0"}}\n' }).p)).toBe(true);
    const old = planWith({ "package.json": '{"dependencies": {"express": "^4.21.0"}}\n' });
    expect(active(old.p)).toBe(false);
    // the pack rules about Express 5 are gated the same way
    expect(old.p.gated.filter((g) => g.id.startsWith("local/"))).toEqual([{ id: "local/v5-only", needs: "express >=5" }]);
    const status = forge(["status"], old.repo).stdout;
    expect(status).toContain("Inactive (version gate):\n");
    expect(status).toContain("\n  local/v5-only: needs express >=5\n");
    expect(active(planWith({ "package.json": '{"dependencies": {}}\n' }).p)).toBe(false);
  });

  test("package.json packageManager declares the package manager's version", () => {
    const gate = GATED.replace('{ecosystem: npm, name: express, version: ">=5"}', '{ecosystem: npm, name: pnpm, version: ">=11 <12"}');
    expect(active(planWith({ "package.json": '{"packageManager": "pnpm@11.2.0+sha512.abc"}\n', ".forge/rules/v5-only.yaml": gate }).p)).toBe(true);
    expect(active(planWith({ "package.json": '{"packageManager": "pnpm@10.18.0"}\n', ".forge/rules/v5-only.yaml": gate }).p)).toBe(false);
  });

  test("in a monorepo, a gated rule applies only in the packages where its gate holds", () => {
    const { p } = planWith({
      "package.json": '{"name": "mono", "private": true, "workspaces": ["apps/*"]}\n',
      "apps/old/package.json": '{"name": "old", "dependencies": {"express": "4.21.2"}}\n',
      "apps/new/package.json": '{"name": "new", "dependencies": {"express": "5.1.0"}}\n',
    });
    const rule = parseStrictYaml(p.files.get(".forge/ast-grep/rules/local/v5-only.yml")!, "rule") as { files?: string[] };
    expect(rule.files).toEqual(["apps/new/**"]);
  });
});

/**
 * A type rule may need packages installed for the types engine to see a library's
 * types at all (Django's come from django-stubs): without them every receiver is
 * Unknown and the rule would pass in silence. Such a rule (`needs` on its ast-grep
 * target) is not enforced where they are not dependencies, and `forge status` says what
 * to add.
 */
test("a type rule whose stubs are not dependencies is not enforced, and status names them", () => {
  const RULE = `id: local/qs-len
description: No len() of a QuerySet.
scope: {language: python}
enforce:
  - target: ast-grep
    mode: advise
    rule: {pattern: "len($Q)"}
    where:
      Q: {type: {package: django, name: QuerySet}}
    needs: [{ecosystem: pypi, name: django-stubs}]
examples:
  deps: [{ecosystem: pypi, name: django, version: "6.1.2"}, {ecosystem: pypi, name: django-stubs, version: "6.1.2"}]
  match: ["x = 1"]
`;
  const planOf = (deps: string) => {
    const repo = mkdtempSync(join(tmpdir(), "forge-needs-"));
    put(repo, "pyproject.toml", `[project]\nname = "app"\ndependencies = ["django>=6.1"]\n\n[dependency-groups]\ndev = [${deps}]\n`);
    put(repo, ".forge/rules/qs-len.yaml", RULE);
    sh("git", ["init", "-q"], { cwd: repo });
    return plan({ level: "project", base: repo, packs: PACKS, ...readProjectInputs(repo) });
  };
  const without = planOf("").active.find((s) => s.rule.id === "local/qs-len")!;
  expect(without.astgrep).toBeUndefined();
  expect(without.notEnforced[0]).toContain("not enforced here: the types engine needs django-stubs installed");
  const withStubs = planOf('"django-stubs>=6.1"').active.find((s) => s.rule.id === "local/qs-len")!;
  expect(withStubs.astgrep).toBeDefined();
});

/**
 * Detection beyond files and dependencies (§5.1): a root pack can be active in every
 * repository (`always`), a pack can be detected by a literal value in a literal config file
 * (`content_any`) or by the structure of a YAML document (`yaml_keys`). File contents are read
 * only for packs that ask, and `forge.yaml` `packs.add` / `packs.remove` still decide last.
 */
describe("detection by content", () => {
  function packsDir(files: Record<string, string>): Map<string, Pack> {
    const dir = mkdtempSync(join(tmpdir(), "forge-detect-packs-"));
    for (const [rel, text] of Object.entries(files)) put(dir, rel, text);
    return loadPacks(dir);
  }
  function activeIn(packs: Map<string, Pack>, files: Record<string, string>): string[] {
    const repo = mkdtempSync(join(tmpdir(), "forge-detect-"));
    for (const [rel, text] of Object.entries(files)) put(repo, rel, text);
    sh("git", ["init", "-q"], { cwd: repo });
    return plan({ level: "project", base: repo, packs, ...readProjectInputs(repo) }).packs.map((p) => p.id);
  }

  test("a root pack with detect.always is active in every repository, with its context file", () => {
    const packs = packsDir({ "everywhere/pack.yaml": "id: everywhere\ndescription: E.\nlevel: project\ndetect: {always: true}\n", "everywhere/context.md": "- A fact.\n" });
    const repo = mkdtempSync(join(tmpdir(), "forge-detect-"));
    sh("git", ["init", "-q"], { cwd: repo });
    const p = plan({ level: "project", base: repo, packs, ...readProjectInputs(repo) });
    expect(p.packs).toEqual([{ id: "everywhere", why: "detected" }]);
    expect(p.files.get(".claude/rules/forge-everywhere.md")).toContain("- A fact.");
    expect(activeIn(packs, { "forge.yaml": 'forge: "0.5"\npacks: {remove: [everywhere]}\n' })).toEqual([]);
  });

  test("detect.always: true is the only value; false is rejected", () => {
    expect(() => packsDir({ "x/pack.yaml": "id: x\ndescription: X.\nlevel: project\ndetect: {always: false}\n" })).toThrow("invalid pack");
  });

  test("detect.yaml_keys: a .yaml or .yml file whose first document has all the keys at its top level", () => {
    const packs = packsDir({ "k8s/pack.yaml": "id: k8s\ndescription: K.\nlevel: project\ndetect: {yaml_keys: [apiVersion, kind]}\n" });
    expect(activeIn(packs, { "deploy/app.yaml": "# a comment\n---\napiVersion: apps/v1\nkind: Deployment\nmetadata: {name: web}\n" })).toEqual(["k8s"]);
    expect(activeIn(packs, { "k/svc.yml": "apiVersion: v1\nkind: Service\n---\nother: 1\n" })).toEqual(["k8s"]);
    // one key only, keys below the top level, a later document, a list, a template that is not YAML, another extension
    expect(activeIn(packs, { "a.yaml": "kind: Deployment\n" })).toEqual([]);
    expect(activeIn(packs, { "b.yaml": "spec:\n  apiVersion: v1\n  kind: Pod\n" })).toEqual([]);
    expect(activeIn(packs, { "c.yaml": "name: x\n---\napiVersion: v1\nkind: Pod\n" })).toEqual([]);
    expect(activeIn(packs, { "d.yaml": "- apiVersion: v1\n  kind: Pod\n" })).toEqual([]);
    expect(activeIn(packs, { "e.yaml": "{{- if .Values.pod }}\napiVersion: v1\nkind: Pod\n{{- end }}\n" })).toEqual([]);
    expect(activeIn(packs, { "f.json": '{"apiVersion": "v1", "kind": "Pod"}\n' })).toEqual([]);
  });

  test("detect.content_any: a line of a literal config file matches the pattern; a comment or another file does not", () => {
    const packs = packsDir({
      "next/pack.yaml": "id: next\ndescription: N.\nlevel: project\ndetect: {files_any: [next.config.*]}\n",
      "next/cc/pack.yaml": 'id: next/cc\ndescription: C.\nlevel: project\ndetect:\n  content_any: [{files: ["next.config.{js,mjs,ts}"], pattern: "^\\\\s*cacheComponents\\\\s*:\\\\s*true\\\\b"}]\n',
    });
    expect(activeIn(packs, { "next.config.ts": "export default {\n  cacheComponents: true,\n};\n" })).toEqual(["next", "next/cc"]);
    expect(activeIn(packs, { "next.config.ts": "export default {\n  // cacheComponents: true,\n  cacheComponents: false,\n};\n" })).toEqual(["next"]);
    expect(activeIn(packs, { "next.config.ts": "export default {};\n", "notes.md": "cacheComponents: true\n" })).toEqual(["next"]);
    expect(activeIn(packs, { "next.config.ts": "export default {};\n", "forge.yaml": 'forge: "0.5"\npacks: {add: [next/cc]}\n' })).toEqual(["next", "next/cc"]);
    expect(activeIn(packs, { "next.config.mjs": "export default {\n  cacheComponents: true };\n", "forge.yaml": 'forge: "0.5"\npacks: {remove: [next/cc]}\n' })).toEqual(["next"]);
  });

  test("content and YAML detection read the files of each workspace package, relative to it", () => {
    const packs = packsDir({
      "next/pack.yaml": "id: next\ndescription: N.\nlevel: project\ndetect: {content_any: [{files: [next.config.ts], pattern: cacheComponents}]}\n",
    });
    const ws = { "package.json": '{"name": "mono", "private": true, "workspaces": ["apps/*"]}\n', "apps/web/package.json": '{"name": "web"}\n' };
    expect(activeIn(packs, { ...ws, "apps/web/next.config.ts": "export default { cacheComponents: true };\n" })).toEqual(["next"]);
    expect(activeIn(packs, { ...ws, "apps/next.config.ts": "export default { cacheComponents: true };\n" })).toEqual([]);
  });

  /** @pack kubernetes */
  test("a Kubernetes manifest activates the kubernetes pack whatever its file is called; a workflow or a compose file does not", () => {
    expect(activeIn(PACKS, { "ops/web.yaml": "apiVersion: apps/v1\nkind: Deployment\nmetadata:\n  name: web\n" })).toContain("kubernetes");
    expect(activeIn(PACKS, { ".github/workflows/ci.yml": "name: ci\non: push\njobs: {}\n", "compose.yaml": "services:\n  db:\n    image: postgres:17\n" })).not.toContain("kubernetes");
  });

  /** @pack typescript/nextjs/cache-components */
  test("a Next.js app whose next.config sets cacheComponents: true activates typescript/nextjs/cache-components; one that does not, or only mentions it in a comment, does not", () => {
    const pkg = { "package.json": '{"name": "web", "dependencies": {"next": "16.2.0"}}\n', "tsconfig.json": "{}\n" };
    expect(activeIn(PACKS, { ...pkg, "next.config.ts": "const config = {\n  cacheComponents: true,\n};\nexport default config;\n" })).toContain("typescript/nextjs/cache-components");
    expect(activeIn(PACKS, { ...pkg, "next.config.ts": "const config = {\n  // cacheComponents: true,\n};\nexport default config;\n" })).not.toContain("typescript/nextjs/cache-components");
  });

  test("a content_any pattern that is not a regular expression is rejected when the packs load", () => {
    expect(() => packsDir({ "x/pack.yaml": 'id: x\ndescription: X.\nlevel: project\ndetect: {content_any: [{files: [a], pattern: "("}]}\n' })).toThrow("is not a regular expression");
  });
});

describe("strict YAML", () => {
  test.each([
    ["duplicate keys", "a: 1\na: 2\n", /unique|duplicate/i],
    ["aliases and anchors", "a: &x 1\nb: *x\n", /alias|anchor/],
    ["explicit tags", "a: !!str 1\n", /tag/],
  ])("YAML with %s is rejected", (_kind, text, msg) => {
    expect(() => parseStrictYaml(text, "t.yaml")).toThrow(msg);
  });
});

/** @pack typescript/astro typescript/astro/react */
test(
  "status shows the whole active tree of a sample with nested packs",
  () => {
    expect(forge(["status"], applied("astro-app")).stdout).toContain(
      "Packs:\n  git (detected)\n  secrets (detected)\n  supply-chain (detected)\n  typescript (detected)\n  typescript/astro (detected)\n  typescript/astro/react (detected)\n  typescript/bun (detected)\n",
    );
  },
  T,
);
