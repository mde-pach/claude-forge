/**
 * @spec 99 · Internals
 *
 * Building blocks the rest relies on: glob matching (Node side) and the shell
 * `case` patterns generated from the same globs must agree; tool names are
 * found in check commands without false hits on paths; and the documentation
 * site reads this test suite and its JUnit results faithfully.
 */
import { describe, expect, test } from "bun:test";
import { readdirSync } from "node:fs";
import { join } from "node:path";
import { allTests, autoPlaced, countTests, parseJUnit, readSpec, selectNodes, specUses, summarize } from "../../docs/src/lib/spec";
import { loadPacks } from "../../src/packs";
import { globMatch } from "../../src/packs";
import { globToCase, toolsInCommand } from "../../src/render";
import { ROOT, sh } from "../helpers";

describe("globs", () => {
  test.each([
    ["**/.env", ".env", true],
    ["**/.env", "apps/api/.env", true],
    ["**/.env", ".env.example", false],
    ["~/.ssh/**", "~/.ssh/id_ed25519", true],
    ["~/.ssh/**", "~/.sshconfig", false],
    ["app/**", "app/page.tsx", true],
    ["playwright.config.*", "playwright.config.ts", true],
    // alternatives are globs too: ** spans directories inside them, and they nest
    ["{**/docs,site}/*.md", "a/b/docs/x.md", true],
    ["{**/docs,site}/*.md", "site/x.md", true],
    ["{**/docs,site}/*.md", "site/a/x.md", false],
    ["*.{ts,{m,c}ts}", "a.mts", true],
  ])("%s matches %s: %p", (g, p, want) => expect(globMatch(g, p)).toBe(want));

  test("globs become shell case patterns", () => {
    expect(globToCase("**/test_*.py")).toEqual(["test_*.py", "*/test_*.py"]);
    expect(globToCase("**/models/**")).toEqual(["models/*", "*/models/*"]);
    expect(globToCase("playwright.config.{ts,js}")).toEqual(["playwright.config.ts", "playwright.config.js"]);
  });

  test("shell case patterns agree with glob matching on sample paths", () => {
    const globs = ["**/test_*.py", "**/models/**", "e2e/**", "app/**", "**/*.test.*", "prisma/schema.prisma", "**/*.astro"];
    const paths = ["test_a.py", "tests/test_a.py", "a/models/x.py", "models/x.py", "e2e/a.spec.ts", "app/x/page.tsx", "src/a.test.ts", "prisma/schema.prisma", "src/app/x.ts", "src/pages/index.astro"];
    for (const g of globs) {
      const pats = globToCase(g).join("|");
      for (const p of paths) {
        const r = sh("sh", ["-c", `case "$1" in ${pats}) echo y ;; *) echo n ;; esac`, "x", p]);
        expect(`${g} ${p} ${r.stdout.trim()}`).toBe(`${g} ${p} ${globMatch(g, p) ? "y" : "n"}`);
      }
    }
  });
});

test("tools are found in check commands, not in paths or package names", () => {
  expect(toolsInCommand('test "$(alembic heads | wc -l)" -le 1')).toEqual(["alembic"]);
  expect(toolsInCommand("bun install --frozen-lockfile")).toEqual(["bun"]);
  expect(toolsInCommand('if grep -qi "pytest-testmon" pyproject.toml; then pytest -q; fi')).toEqual(["pytest"]);
  expect(toolsInCommand("prisma migrate diff --from-migrations prisma/migrations --to-schema prisma/schema.prisma")).toEqual(["prisma"]);
  expect(toolsInCommand("astro check")).toEqual(["astro"]);
});

describe("the documentation is the test suite", () => {
  test("every test is shown in the documentation: placed on a page with <Spec>, or on the page of a rule or pack it covers", () => {
    const sections = readSpec(ROOT);
    const placed = new Set<string>();
    for (const use of specUses(ROOT)) {
      const section = sections.find((x) => x.slug === use.file);
      expect(section ? use.file : `${use.page}: <Spec file="${use.file}"> names no spec file`).toBe(use.file);
      const nodes = selectNodes(section!, use);
      expect(nodes.length ? "ok" : `${use.page}: <Spec file="${use.file}" describe="${use.describe ?? ""}" test="${use.test ?? ""}"> selects nothing`).toBe("ok");
      for (const t of allTests(nodes)) placed.add(`${section!.file}:${t.test.line}`);
    }
    const missing = sections.flatMap((s) =>
      allTests(s.children)
        .filter((t) => !placed.has(`${s.file}:${t.test.line}`) && !autoPlaced(t.test))
        .map((t) => `${s.file}:${t.test.line} ${[...t.path, t.test.title].join(" > ")}`),
    );
    expect(missing).toEqual([]);
  });

  test("@rule and @pack tags name rules and packs that exist", () => {
    const packs = loadPacks(join(ROOT, "packs"));
    const rules = new Set([...packs.values()].flatMap((p) => p.rules.map((r) => r.id)));
    const unknown = readSpec(ROOT).flatMap((s) =>
      allTests(s.children).flatMap(({ test }) => [...test.rules.filter((r) => !rules.has(r)).map((r) => `@rule ${r}`), ...test.packs.filter((p) => !packs.has(p)).map((p) => `@pack ${p}`)]),
    );
    expect(unknown).toEqual([]);
  });

  test("every spec file has an @spec header, and the docs extractor finds its tests at the lines bun reports", () => {
    const sections = readSpec(ROOT);
    expect(sections.map((s) => s.file).sort()).toEqual(
      readdirSync(join(ROOT, "tests/spec"))
        .filter((f) => f.endsWith(".test.ts"))
        .map((f) => `tests/spec/${f}`)
        .sort(),
    );
    for (const s of sections) {
      expect(s.title.length).toBeGreaterThan(0);
      expect(countTests(s.children)).toBeGreaterThan(0);
    }
    // this very test, found where bun will report it
    const self = sections.find((s) => s.slug === "99-internals")!;
    const describe = self.children.find((n) => n.kind === "describe" && n.title === "the documentation is the test suite");
    const me = describe?.kind === "describe" ? describe.children.find((n) => n.kind === "test" && n.title.startsWith("every spec file has an @spec header")) : undefined;
    expect(me?.kind === "test" ? me.code : "").toContain("this very test");
  });

  test("JUnit results are keyed by file and line, with every parameterised case", () => {
    const xml = `<testsuites><testsuite name="a"><testcase name="x 1" classname="g" time="0.1" file="tests/spec/a.test.ts" line="7" /><testcase name="x 2" classname="g" time="0" file="tests/spec/a.test.ts" line="7"><failure type="E" message="boom &amp; more">trace</failure></testcase><testcase name="y" file="tests/spec/a.test.ts" line="9"><skipped /></testcase></testsuite></testsuites>`;
    const r = parseJUnit(xml, "linux");
    expect(r.get("tests/spec/a.test.ts:7")!.map((x) => [x.name, x.status, x.message])).toEqual([
      ["x 1", "passed", undefined],
      ["x 2", "failed", "boom & more"],
    ]);
    expect(summarize(r.get("tests/spec/a.test.ts:7")!)).toBe("failed");
    expect(summarize(r.get("tests/spec/a.test.ts:9")!)).toBe("skipped");
    expect(summarize([])).toBe("not run");
  });
});
