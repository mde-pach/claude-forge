import { describe, expect, test } from "bun:test";
import { join } from "node:path";
import { examplePath } from "../src/audit";
import { globMatch, loadPacks } from "../src/packs";
import { globToCase, toolsInCommand } from "../src/render";
import { permissionMatches, selfTest } from "../src/selftest";
import { addEntries, removeEntries } from "../src/settings";
import { parseStrictYaml } from "../src/yaml";
import { ROOT, sh } from "./helpers";
import type { Rule } from "../src/types";

describe("strict YAML", () => {
  test.each([
    ["a: 1\na: 2\n", /unique|duplicate/i],
    ["a: &x 1\nb: *x\n", /alias|anchor/],
    ["a: !!str 1\n", /tag/],
  ])("rejects %j", (text, msg) => {
    expect(() => parseStrictYaml(text, "t.yaml")).toThrow(msg);
  });
  test("accepts plain YAML", () => expect(parseStrictYaml("a: [1, {b: c}]\n", "t.yaml")).toEqual({ a: [1, { b: "c" }] }));
});

describe("settings (JSONC, comments preserved)", () => {
  const base = '{\n  // mine\n  "permissions": {\n    "deny": ["Read(x)"] // keep\n  }\n}\n';
  test("appends, skips existing, records only what it added; remove restores the values", () => {
    const { text, added } = addEntries(base, [
      { file: "s", path: "permissions.deny", value: "Read(x)", append: true },
      { file: "s", path: "permissions.ask", value: "Edit(.forge/**)", append: true },
      { file: "s", path: "sandbox.enabled", value: true, append: false },
    ]);
    expect(text).toContain("// mine");
    expect(text).toContain("// keep");
    expect(added.map((e) => e.path)).toEqual(["permissions.ask", "sandbox.enabled"]);
    const back = removeEntries(text, added)!;
    expect(back).toContain("// mine");
    expect(JSON.parse(back.replace(/\/\/.*$/gm, ""))).toEqual({ permissions: { deny: ["Read(x)"] } });
  });
  test("a scalar already set is left alone", () => {
    const { added } = addEntries('{"sandbox": {"enabled": false}}', [{ file: "s", path: "sandbox.enabled", value: true, append: false }]);
    expect(added).toEqual([]);
  });
  test("removal that empties the file returns undefined", () => {
    const { text, added } = addEntries("", [{ file: "s", path: "permissions.ask", value: "Edit(a)", append: true }]);
    expect(removeEntries(text, added)).toBeUndefined();
  });
});

describe("globs", () => {
  test.each([
    ["**/.env", ".env", true],
    ["**/.env", "apps/api/.env", true],
    ["**/.env", ".env.example", false],
    ["~/.ssh/**", "~/.ssh/id_ed25519", true],
    ["~/.ssh/**", "~/.sshconfig", false],
    ["app/**", "app/page.tsx", true],
    ["playwright.config.*", "playwright.config.ts", true],
  ])("%s ~ %s = %p", (g, p, want) => expect(globMatch(g, p)).toBe(want));

  test("shell case patterns", () => {
    expect(globToCase("**/test_*.py")).toEqual(["test_*.py", "*/test_*.py"]);
    expect(globToCase("**/models/**")).toEqual(["models/*", "*/models/*"]);
    expect(globToCase("e2e/**")).toEqual(["e2e/*"]);
    expect(globToCase("playwright.config.{ts,js}")).toEqual(["playwright.config.ts", "playwright.config.js"]);
  });

  test("case patterns agree with globMatch on sample paths", () => {
    const globs = ["**/test_*.py", "**/models/**", "e2e/**", "app/**", "**/*.test.*", "prisma/schema.prisma"];
    const paths = ["test_a.py", "tests/test_a.py", "a/models/x.py", "models/x.py", "e2e/a.spec.ts", "app/x/page.tsx", "src/a.test.ts", "prisma/schema.prisma", "src/app/x.ts"];
    for (const g of globs) {
      const pats = globToCase(g).join("|");
      for (const p of paths) {
        const r = sh("sh", ["-c", `case "$1" in ${pats}) echo y ;; *) echo n ;; esac`, "x", p]);
        expect(`${g} ${p} ${r.stdout.trim()}`).toBe(`${g} ${p} ${globMatch(g, p) ? "y" : "n"}`);
      }
    }
  });
});

test("tools in commands", () => {
  expect(toolsInCommand('test "$(alembic heads | wc -l)" -le 1')).toEqual(["alembic"]);
  expect(toolsInCommand("bun install --frozen-lockfile")).toEqual(["bun"]);
  expect(toolsInCommand('if grep -qi "pytest-testmon" pyproject.toml; then pytest -q; fi')).toEqual(["pytest"]);
  expect(toolsInCommand("if ! grep -q shadowDatabaseUrl prisma.config.ts; then echo x; else prisma migrate diff --from-migrations prisma/migrations; fi")).toEqual(["prisma"]);
});

test("permission matcher", () => {
  expect(permissionMatches("Bash(git clean:*)", "Bash(git clean -fd)")).toBe(true);
  expect(permissionMatches("Bash(git clean:*)", "Bash(git clean)")).toBe(true);
  expect(permissionMatches("Bash(git clean:*)", "Bash(git cleanup-branches)")).toBe(false);
  expect(permissionMatches("Read(**/.env)", "Edit(.env)")).toBe(false);
});

test("audit example paths stay inside the rule scope", () => {
  const rule = (language: Rule["scope"]["language"], files?: string[]) => ({ scope: { language, files } }) as Rule;
  expect(examplePath(rule("python"), "f")).toBe("f.py");
  expect(examplePath(rule("python", ["**/test_*.py"]), "f")).toBe("test_f.py");
  expect(examplePath(rule("typescript", ["app/**"]), "f")).toBe("app/f.ts");
  expect(examplePath(rule("typescript", ["**/*.test.*"]), "f")).toBe("f.test.ts");
});

test(
  "forge test: packs, rule-tests, guard and permission examples, context.md",
  () => {
    const r = selfTest(join(ROOT, "packs"));
    expect(r.failures).toEqual([]);
    expect(r.counts.rules).toBe([...loadPacks(join(ROOT, "packs")).values()].reduce((n, p) => n + p.rules.length, 0));
  },
  300_000,
);
