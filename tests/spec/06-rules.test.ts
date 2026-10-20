/**
 * @spec 6 · Rules and targets
 *
 * A rule lists ordered **targets**, each compiled to the closest native place:
 * the project's own tools (`tool`, audited), ast-grep, Claude Code permissions
 * and the sandbox, then hooks (`guard`, `check`). A `tool` target followed by an
 * `ast-grep` target falls back to ast-grep only when the project does not use
 * that tool. Every rule blocks; a rule you don't want is disabled with a reason.
 */
import { describe, expect, test } from "bun:test";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { applied, forge, git } from "../helpers";

const T = 120_000;
const read = (repo: string, rel: string) => readFileSync(join(repo, rel), "utf8");

describe("ast-grep targets", () => {
  test(
    "each ast-grep target becomes .forge/ast-grep/rules/<rule-id>.yml with the rule's scope, and its examples become a rule test",
    () => {
      const repo = applied("nextjs-prisma-app");
      const rule = read(repo, ".forge/ast-grep/rules/typescript/nextjs/no-pages-data.yml");
      expect(rule).toContain("id: typescript/nextjs/no-pages-data\nlanguage: Tsx\nseverity: error\n");
      expect(rule).toContain("message: forge:typescript/nextjs/no-pages-data ");
      expect(rule).toContain("files:\n  - app/**\n  - src/app/**\n");
      const t = read(repo, ".forge/ast-grep/rule-tests/typescript-nextjs-no-pages-data-test.yml");
      expect(t).toContain("id: typescript/nextjs/no-pages-data\nvalid:");
    },
    T,
  );

  test(
    "scope.files entries starting with ! become ast-grep ignores",
    () => {
      const rule = read(applied("fastapi-app"), ".forge/ast-grep/rules/python/pytest/no-test-env-branch.yml");
      expect(rule).toContain("files:\n  - \"**/*.py\"\nignores:\n  - \"**/test_*.py\"");
    },
    T,
  );

  test(
    "typescript rules parse .ts and .tsx with one Tsx rule file",
    () => {
      expect(read(applied("eslint-app"), ".forge/ast-grep/sgconfig.yml")).toContain('languageGlobs:\n  tsx: ["*.ts", "*.mts", "*.cts"]');
    },
    T,
  );
});

describe("tool targets and their fallback", () => {
  test(
    "with Biome configured, no-explicit-any is enforced by Biome (audited) and no ast-grep rule is generated",
    () => {
      const repo = applied("nextjs-prisma-app");
      expect(existsSync(join(repo, ".forge/ast-grep/rules/typescript/no-explicit-any.yml"))).toBe(false);
      expect(forge(["status"], repo).stdout).toContain("typescript/no-explicit-any\n    biome (lint/suspicious/noExplicitAny), audited");
    },
    T,
  );

  test(
    "with ESLint instead of Biome, no-explicit-any runs through its ast-grep target and the Biome check is not enforced here",
    () => {
      const repo = applied("eslint-app");
      expect(existsSync(join(repo, ".forge/ast-grep/rules/typescript/no-explicit-any.yml"))).toBe(true);
      expect(read(repo, ".forge/check.sh")).not.toContain("biome");
      expect(forge(["status"], repo).stdout).toContain("typescript/format-lint\n    not enforced here: biome not configured");
      expect(git(repo, "status", "--porcelain").stdout.trim().split("\n").sort()).toEqual(["?? .claude/", "?? .forge/"]);
    },
    T,
  );
});

describe("permission, guard and context targets", () => {
  test(
    "permission targets of active project packs are appended to permissions.<decision>, with the setup's own ask entries",
    () => {
      const settings = JSON.parse(read(applied("fastapi-app"), ".claude/settings.json"));
      expect(settings.permissions.ask).toEqual(["Edit(.forge/**)", "Edit(forge.yaml)", "Edit(.claude/skills/forge-project/**)", "Edit(.claude/rules/forge-*.md)"]);
    },
    T,
  );

  test(
    "guard targets become guard.rules lines, deny before ask, with a header naming the source rule files",
    () => {
      const rules = read(applied("fastapi-app"), ".claude/skills/forge-project/scripts/guard.rules").split("\n");
      expect(rules[2]).toBe("# Sources: packs/python/sqlalchemy/alembic/rules/downgrade.yaml, packs/python/sqlalchemy/alembic/rules/no-downgrade-base.yaml, packs/python/uv/rules/no-pip.yaml");
      expect(rules.slice(3, 6).map((l) => l.split("\t").slice(0, 2).join(" "))).toEqual([
        "python/sqlalchemy/alembic/no-downgrade-base deny",
        "python/sqlalchemy/alembic/downgrade ask",
        "python/uv/no-pip ask",
      ]);
    },
    T,
  );

  test(
    "each active pack with a context.md produces .claude/rules/forge-<pack-id>.md with the same content",
    () => {
      const repo = applied("nextjs-prisma-app");
      expect(read(repo, ".claude/rules/forge-typescript-nextjs-react.md")).toBe(read(join(import.meta.dir, "../.."), "packs/typescript/nextjs/react/context.md"));
      expect(read(repo, ".claude/rules/forge-typescript-nextjs-react.md")).not.toContain("paths:");
    },
    T,
  );
});

describe("check targets", () => {
  test(
    "check targets become functions of check.sh; project tools in their commands resolve to the project's own binaries",
    () => {
      const check = read(applied("astro-app"), ".forge/check.sh");
      expect(check).toContain("astro() { forge_exec_tool astro \"$@\"; }");
      expect(check).toContain("c_id='typescript/astro/check'; c_stage=typecheck; c_tools='astro'");
      expect(check).toContain("forge_check_run_");
    },
    T,
  );
});
