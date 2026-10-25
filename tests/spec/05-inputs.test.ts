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
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { lineage, loadPacks } from "../../src/packs";
import { plan, readProjectInputs } from "../../src/plan";
import { parseStrictYaml } from "../../src/yaml";
import { applied, forge, put, ROOT, sampleRepo, sh } from "../helpers";

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
    expect(astro).toEqual(["typescript", "typescript/astro", "typescript/astro/react", "typescript/bun", "typescript/react"]);
  });

  test("detection follows dependencies with versions, and lock files refine them", () => {
    expect(activePacks("fastapi-app")).toEqual([
      "python",
      "python/fastapi",
      "python/pydantic",
      "python/pytest",
      "python/sqlalchemy",
      "python/sqlalchemy/alembic",
      "python/uv",
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
    expect(p.packs.map((x) => x.id)).toEqual(["typescript", "typescript/vitest"]);
  });

  test("package.json workspaces (npm, Yarn, Bun) and uv workspace members declare packages too; excluded ones are not read", () => {
    const npm = planOf({ "package.json": '{"name": "mono", "workspaces": ["apps/*"]}\n', ...inDir("apps/web", TS), ...inDir("other/x", { "package.json": '{"dependencies": {"zod": "4.0.0"}}\n' }) });
    expect(npm.packs.map((x) => x.id)).toEqual(["typescript", "typescript/vitest"]);
    const yarn = planOf({ "package.json": '{"name": "mono", "workspaces": {"packages": ["apps/*"]}}\n', ...inDir("apps/web", TS) });
    expect(yarn.packs.map((x) => x.id)).toContain("typescript");
    const uv = planOf({
      "pyproject.toml": '[project]\nname = "mono"\n\n[tool.uv.workspace]\nmembers = ["services/*"]\nexclude = ["services/old"]\n',
      "services/api/pyproject.toml": '[project]\nname = "api"\ndependencies = ["fastapi>=0.115"]\n',
      "services/old/pyproject.toml": '[project]\nname = "old"\ndependencies = ["pytest>=8"]\n',
    });
    expect(uv.packs.map((x) => x.id)).toEqual(["python", "python/fastapi", "python/pydantic"]);
  });

  test("without a workspace declaration, packages/* are read", () => {
    expect(planOf(inDir("packages/lib", TS)).packs.map((x) => x.id)).toEqual(["typescript", "typescript/vitest"]);
  });

  test("a check whose tool is configured in a package runs there; at the root, as before, when the root configures it", () => {
    const ws = planOf({ "package.json": '{"name": "mono"}\n', "pnpm-workspace.yaml": "packages: [packages/*]\n", ...inDir("packages/a", TS), ...inDir("packages/b", TS) });
    const typecheck = ws.active.find((s) => s.rule.id === "typescript/typecheck")!;
    const TSC = 'tsc --noEmit -p tsconfig.json --pretty false --incremental --tsBuildInfoFile "$(forge_state_file tsc.tsbuildinfo)"';
    expect(typecheck.enforced).toEqual([`check typecheck: ${TSC} (in packages/a, packages/b)`]);
    expect(ws.files.get(".forge/check.sh")).toContain("c_dirs='packages/a packages/b'");
    const root = planOf({ ...TS, "pnpm-workspace.yaml": "packages: [packages/*]\n", ...inDir("packages/a", TS) });
    expect(root.active.find((s) => s.rule.id === "typescript/typecheck")!.enforced).toEqual([`check typecheck: ${TSC}`]);
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
      "Packs:\n  typescript (detected)\n  typescript/astro (detected)\n  typescript/astro/react (detected)\n  typescript/bun (detected)\n",
    );
  },
  T,
);
