/**
 * @spec 11 · Tool audits
 *
 * For each `tool` target of an active rule, Forge runs the project's own tool
 * with the project's own config on the rule's examples. Every `match` example
 * must get the expected diagnostic **at a level that fails the tool's run**
 * (an error, or a warning when the tool fails on warnings), and no `no_match`
 * example may get it; then the rule is satisfied, and stricter configs satisfy
 * it automatically. A diagnostic that is only a warning does not enforce
 * anything: the rule is unsatisfied. Severity is read from the tools'
 * machine-readable output (Biome's JSON reporter, `ruff --output-format json`,
 * `basedpyright --outputjson`; tsc only has errors). Otherwise `forge status`
 * prints the snippet to add. Forge never edits a tool config.
 * These tests install real Biome, TypeScript, ruff and basedpyright once
 * (versions pinned in tests/helpers.ts).
 */
import { beforeAll, describe, expect, test } from "bun:test";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { auditTarget, examplePath } from "../../src/audit";
import { loadPacks } from "../../src/packs";
import { commitAll, ensureTools, forge, git, hasBun, hasUv, linkTools, ROOT, sampleRepo } from "../helpers";
import type { Rule, ToolTarget } from "../../src/types";

const T = 300_000;

beforeAll(ensureTools, T);

/** A sample repo with the cached tools linked in and the given config files, committed. */
function repo(name: string, files: Record<string, string>): string {
  const dir = sampleRepo(name);
  for (const [rel, text] of Object.entries(files)) writeFileSync(join(dir, rel), text);
  linkTools(dir, name === "fastapi-app" ? "python" : "node");
  commitAll(dir, "config");
  return dir;
}

/** forge status output; also asserts the audit left no file behind. */
function audit(dir: string): string {
  const r = forge(["status"], dir);
  expect(r.status).toBe(0);
  expect(git(dir, "status", "--porcelain").stdout).toBe("");
  return r.stdout;
}

const BIOME = (rules: object) =>
  JSON.stringify({ vcs: { enabled: true, clientKind: "git", useIgnoreFile: true }, linter: { enabled: true, rules: { recommended: false, ...rules } } });
const TSCONFIG = (strict: boolean) => JSON.stringify({ compilerOptions: { strict, noEmit: true, skipLibCheck: true }, include: ["src"] });

describe.if(hasBun)("Biome and tsc", () => {
  /** @rule typescript/no-explicit-any typescript/no-ts-ignore typescript/no-implicit-any typescript/strict-null-checks typescript/strict-function-types */
  test(
    "a correct config is satisfied: Biome rules on and strict tsconfig",
    () => {
      const out = audit(repo("nextjs-prisma-app", { "biome.json": BIOME({ suspicious: { noExplicitAny: "error", noTsIgnore: "error" } }), "tsconfig.json": TSCONFIG(true) }));
      expect(out).toContain("forge:typescript/no-explicit-any biome lint/suspicious/noExplicitAny: satisfied");
      expect(out).toContain("forge:typescript/no-ts-ignore biome lint/suspicious/noTsIgnore: satisfied");
      expect(out).toContain("forge:typescript/no-implicit-any tsc TS7006: satisfied");
      expect(out).toContain("forge:typescript/strict-null-checks tsc TS18047: satisfied");
      expect(out).toContain("forge:typescript/strict-function-types tsc TS2322: satisfied");
    },
    T,
  );

  /** @rule typescript/no-explicit-any typescript/no-ts-ignore typescript/no-implicit-any typescript/strict-null-checks typescript/strict-function-types */
  test(
    "an incorrect config is unsatisfied and status prints the snippet to add",
    () => {
      const out = audit(repo("nextjs-prisma-app", { "biome.json": BIOME({ suspicious: { noExplicitAny: "off", noTsIgnore: "off" } }), "tsconfig.json": TSCONFIG(false) }));
      expect(out).toContain("forge:typescript/no-explicit-any biome lint/suspicious/noExplicitAny: unsatisfied");
      expect(out).toContain('add to the biome config: "linter": {"rules": {"suspicious": {"noExplicitAny": "error"}}}');
      expect(out).toContain("forge:typescript/no-ts-ignore biome lint/suspicious/noTsIgnore: unsatisfied");
      expect(out).toContain("forge:typescript/no-implicit-any tsc TS7006: unsatisfied");
      expect(out).toContain("forge:typescript/strict-null-checks tsc TS18047: unsatisfied");
      expect(out).toContain("forge:typescript/strict-function-types tsc TS2322: unsatisfied");
      expect(out).toContain('add to the tsc config: "compilerOptions": {"strict": true}');
    },
    T,
  );

  /** @rule typescript/no-explicit-any */
  test(
    "Biome's recommended set reports noExplicitAny only as a warning, which does not fail biome ci: unsatisfied",
    () => {
      const out = audit(repo("nextjs-prisma-app", { "biome.json": JSON.stringify({ linter: { enabled: true, rules: { recommended: true } } }), "tsconfig.json": TSCONFIG(true) }));
      expect(out).toContain(
        "forge:typescript/no-explicit-any biome lint/suspicious/noExplicitAny: unsatisfied (lint/suspicious/noExplicitAny is only a warning in your biome config, so biome does not fail on it",
      );
      expect(out).toContain('add to the biome config: "linter": {"rules": {"suspicious": {"noExplicitAny": "error"}}}');
    },
    T,
  );

  test(
    "apply prints the unsatisfied audits",
    () => {
      const r = forge(["apply", "--yes"], repo("nextjs-prisma-app", { "biome.json": BIOME({}), "tsconfig.json": TSCONFIG(true) }));
      expect(r.status).toBe(0);
      expect(r.stdout).toContain("noExplicitAny: unsatisfied");
    },
    T,
  );
});

describe.if(hasUv)("ruff and basedpyright", () => {
  const PYPROJECT = (select: string[], any: string, failOnWarnings?: boolean) =>
    `[project]\nname = "fastapi-app"\nversion = "0.1.0"\ndependencies = ["fastapi>=0.115", "sqlalchemy>=2.0.36", "alembic>=1.14", "pydantic>=2.9"]\n\n[dependency-groups]\ndev = ["pytest>=8.3", "ruff>=0.8", "basedpyright>=1.22"]\n\n[tool.ruff.lint]\nselect = ${JSON.stringify(select)}\n\n[tool.basedpyright]\nreportExplicitAny = "${any}"\n${failOnWarnings === undefined ? "" : `failOnWarnings = ${failOnWarnings}\n`}`;

  /** @rule python/no-bare-except python/no-explicit-any */
  test(
    "a correct config is satisfied: E722 and ANN401 selected, reportExplicitAny on",
    () => {
      const out = audit(repo("fastapi-app", { "pyproject.toml": PYPROJECT(["E", "ANN401"], "error") }));
      expect(out).toContain("forge:python/no-bare-except ruff E722: satisfied");
      expect(out).toContain("forge:python/no-explicit-any ruff ANN401: satisfied");
      expect(out).toContain("forge:python/no-explicit-any basedpyright reportExplicitAny: satisfied");
    },
    T,
  );

  /** @rule python/no-bare-except python/no-explicit-any */
  test(
    "an incorrect config is unsatisfied and status prints the snippet to add",
    () => {
      const out = audit(repo("fastapi-app", { "pyproject.toml": PYPROJECT(["F"], "none") }));
      expect(out).toContain("forge:python/no-bare-except ruff E722: unsatisfied");
      expect(out).toContain('add to the ruff config: [tool.ruff.lint] extend-select = ["E722"]');
      expect(out).toContain("forge:python/no-explicit-any ruff ANN401: unsatisfied");
      expect(out).toContain("forge:python/no-explicit-any basedpyright reportExplicitAny: unsatisfied");
    },
    T,
  );

  /** @rule python/no-explicit-any */
  test(
    "reportExplicitAny as a warning, with failOnWarnings off, does not fail basedpyright: unsatisfied",
    () => {
      const out = audit(repo("fastapi-app", { "pyproject.toml": PYPROJECT(["E", "ANN401"], "warning", false) }));
      expect(out).toContain(
        "forge:python/no-explicit-any basedpyright reportExplicitAny: unsatisfied (reportExplicitAny is only a warning in your basedpyright config, so basedpyright does not fail on it",
      );
      expect(out).toContain('add to the basedpyright config: [tool.basedpyright] reportExplicitAny = "error"');
    },
    T,
  );

  /** @rule python/no-explicit-any */
  test(
    "reportExplicitAny as a warning, with failOnWarnings on, fails basedpyright: satisfied",
    () => {
      const out = audit(repo("fastapi-app", { "pyproject.toml": PYPROJECT(["E", "ANN401"], "warning", true) }));
      expect(out).toContain("forge:python/no-explicit-any basedpyright reportExplicitAny: satisfied");
    },
    T,
  );

  /** @rule python/no-bare-except python/no-explicit-any */
  test(
    "a stricter config satisfies the rule automatically (select ALL)",
    () => {
      const out = audit(repo("fastapi-app", { "pyproject.toml": PYPROJECT(["ALL"], "error") }));
      expect(out).toContain("forge:python/no-bare-except ruff E722: satisfied");
      expect(out).toContain("forge:python/no-explicit-any ruff ANN401: satisfied");
    },
    T,
  );
});

test(
  "a tool that is configured but not installed is skipped, never reported satisfied",
  () => {
    expect(forge(["status"], sampleRepo("nextjs-prisma-app")).stdout).toContain(
      "biome lint/suspicious/noExplicitAny: skipped (biome is configured but not installed",
    );
  },
  T,
);

test("examples are written to a temporary file inside the rule's scope", () => {
  const rule = (language: Rule["scope"]["language"], files?: string[]) => ({ scope: { language, files } }) as Rule;
  expect(examplePath(rule("python"), "f")).toBe("f.py");
  expect(examplePath(rule("python", ["**/test_*.py"]), "f")).toBe("test_f.py");
  expect(examplePath(rule("typescript", ["app/**"]), "f")).toBe("app/f.ts");
  expect(examplePath(rule("typescript", ["**/*.test.*"]), "f")).toBe("f.test.ts");
});

/** A project whose config sets exactly the target's diagnostic to an error (on) or turns it off. */
function projectFor(t: ToolTarget, on: boolean): string {
  const dir = mkdtempSync(join(tmpdir(), "forge-audit-"));
  if (t.tool === "biome") {
    const [, group, name] = t.expects.split("/");
    writeFileSync(join(dir, "biome.json"), JSON.stringify({ linter: { enabled: true, rules: { recommended: false, [group!]: { [name!]: on ? "error" : "off" } } } }));
  } else if (t.tool === "tsc") {
    writeFileSync(join(dir, "tsconfig.json"), JSON.stringify({ compilerOptions: { strict: on, noEmit: true, skipLibCheck: true } }));
  } else {
    const ruff = `[tool.ruff.lint]\nselect = ["${t.tool === "ruff" && on ? t.expects : "F"}"]\n`;
    const pyright = `[tool.basedpyright]\npythonVersion = "3.12"\n${t.tool === "basedpyright" ? `${t.expects} = "${on ? "error" : "none"}"\n` : ""}`;
    writeFileSync(join(dir, "pyproject.toml"), `[project]\nname = "audit"\nversion = "0.1.0"\n\n${ruff}\n${pyright}`);
  }
  linkTools(dir, t.tool === "ruff" || t.tool === "basedpyright" ? "python" : "node");
  return dir;
}

const toolRules = [...loadPacks(join(ROOT, "packs")).values()].flatMap((p) => p.rules).filter((r) => r.enforce.some((t) => t.target === "tool"));

describe.if(hasBun && hasUv)("every tool target", () => {
  test.each(toolRules.map((r) => [r.id]))(
    "%s: the examples prove each tool target: satisfied when the tool reports the diagnostic as an error, unsatisfied when it is off",
    (id) => {
      const rule = toolRules.find((r) => r.id === id)!;
      for (const t of rule.enforce.filter((x): x is ToolTarget => x.target === "tool"))
        for (const on of [true, false]) {
          const r = auditTarget(projectFor(t, on), rule, t);
          expect(`${t.tool} ${t.expects} ${on ? "on" : "off"}: ${r.status}${r.detail ? ` (${r.detail})` : ""}`).toBe(`${t.tool} ${t.expects} ${on ? "on" : "off"}: ${on ? "satisfied" : "unsatisfied"}${r.detail && !on ? ` (${r.detail})` : ""}`);
        }
    },
    T,
  );
});
