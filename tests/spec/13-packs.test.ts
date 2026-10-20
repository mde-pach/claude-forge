/**
 * @spec 13 · Packs and rules (forge test)
 *
 * Every pack and rule validates against its schema, and every rule's examples
 * behave as declared: ast-grep rule tests pass (`match` is invalid code,
 * `no_match` valid), guard examples get the rule's decision on every shell,
 * permission examples match the rule's permission strings. There is one test
 * per rule below; the pack pages of this site show the same results.
 */
import { beforeAll, describe, expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { loadPacks } from "../../src/packs";
import { permissionMatches, selfTest, type TestReport } from "../../src/selftest";
import { forge, ROOT } from "../helpers";

const T = 300_000;
const packs = loadPacks(join(ROOT, "packs"));
const rules = [...packs.values()].flatMap((p) => p.rules);
let report: TestReport;

beforeAll(() => {
  report = selfTest(join(ROOT, "packs"));
}, T);

/** Failures of `forge test` that concern one rule. */
const failuresOf = (id: string) => report.failures.filter((f) => f.startsWith(`${id}:`) || f.startsWith(`${id} [`) || (f.startsWith("ast-grep") && f.includes(` ${id} `)));

describe("every rule", () => {
  test.each(rules.map((r) => [r.id]))("%s: examples behave as declared", (id) => {
    expect(failuresOf(id)).toEqual([]);
  });
});

describe("every pack", () => {
  test.each([...packs.values()].filter((p) => p.context !== undefined).map((p) => [p.id]))("%s: context.md is at most 10 lines", (id) => {
    expect(packs.get(id)!.context!.replace(/\n$/, "").split("\n").length).toBeLessThanOrEqual(10);
  });
});

test("forge test passes on the shipped packs and counts every rule file", () => {
  expect(report.failures).toEqual([]);
  expect(report.counts.rules).toBe(rules.length);
});

test(
  "forge test from the CLI summarises and exits 0",
  () => {
    const r = forge(["test"], ROOT);
    expect(r.status).toBe(0);
    expect(r.stdout).toContain(`${packs.size} packs, ${rules.length} rules`);
  },
  T,
);

describe("forge test catches broken packs", () => {
  function packsWith(files: Record<string, string>): string {
    const dir = mkdtempSync(join(tmpdir(), "forge-packs-"));
    for (const [rel, text] of Object.entries({ "demo/pack.yaml": "id: demo\ndescription: Demo.\nlevel: project\n", ...files })) {
      mkdirSync(join(dir, rel, ".."), { recursive: true });
      writeFileSync(join(dir, rel), text);
    }
    return dir;
  }
  const guardRule = (match: string, noMatch: string) =>
    `id: demo/g\ndescription: Demo guard.\nscope: {language: any}\nenforce: [{target: guard, decision: deny, pattern: "rm[[:space:]]+-rf"}]\nexamples: {match: [${JSON.stringify(match)}], no_match: [${JSON.stringify(noMatch)}]}\n`;

  test("a correct pack passes", () => {
    expect(selfTest(packsWith({ "demo/rules/g.yaml": guardRule("rm -rf x", "rm x") })).failures).toEqual([]);
  });
  test("a guard match example that does not trigger fails, per shell", () => {
    const f = selfTest(packsWith({ "demo/rules/g.yaml": guardRule("rm -r x", "rm x") })).failures;
    expect(f).toContain('demo/g [sh]: expected deny for "rm -r x", got nothing');
  });
  test("a guard no_match example that triggers fails", () => {
    expect(selfTest(packsWith({ "demo/rules/g.yaml": guardRule("rm -rf x", "rm -rf y") })).failures.some((x) => x.includes('expected nothing for "rm -rf y"'))).toBe(true);
  });
  test("a permission example that no permission string matches fails", () => {
    const rule = 'id: demo/p\ndescription: P.\nscope: {language: any}\nenforce: [{target: permission, decision: deny, rule: "Bash(git clean:*)"}]\nexamples: {match: ["Bash(git cleanup)"], no_match: ["Bash(ls)"]}\n';
    expect(selfTest(packsWith({ "demo/rules/p.yaml": rule })).failures).toContain("demo/p: no permission rule matches Bash(git cleanup)");
  });
  test(
    "an ast-grep rule whose examples fail fails",
    () => {
      const rule = 'id: demo/a\ndescription: A.\nscope: {language: python}\nenforce: [{target: ast-grep, mode: forbid, rule: {pattern: "eval($$$)"}}]\nexamples: {match: ["exec(x)"], no_match: ["print(x)"]}\n';
      expect(selfTest(packsWith({ "demo/rules/a.yaml": rule })).failures.some((x) => x.startsWith("ast-grep test failed") && x.includes("demo/a"))).toBe(true);
    },
    T,
  );
  test("a context.md over 10 lines fails", () => {
    expect(selfTest(packsWith({ "demo/context.md": "line\n".repeat(11) })).failures).toContain("demo/context.md: 11 lines (max 10)");
  });
  test("a rule id that does not match its file name fails", () => {
    expect(selfTest(packsWith({ "demo/rules/other.yaml": guardRule("rm -rf x", "rm x") })).failures[0]).toContain('id must be "demo/other"');
  });
  test("a rule without examples fails", () => {
    const rule = 'id: demo/n\ndescription: N.\nscope: {language: any}\nenforce: [{target: guard, decision: ask, pattern: "x"}]\n';
    expect(selfTest(packsWith({ "demo/rules/n.yaml": rule })).failures).toContain("demo/n: needs match and no_match examples");
  });
  test("a pack requiring an unknown pack fails", () => {
    expect(selfTest(packsWith({ "demo/pack.yaml": "id: demo\ndescription: D.\nlevel: project\nrequires: [ghost]\n" })).failures[0]).toContain("requires unknown pack ghost");
  });
  test("a user-level pack with detect fails", () => {
    expect(selfTest(packsWith({ "demo/pack.yaml": "id: demo\ndescription: D.\nlevel: user\ndetect: {files_any: [x]}\n" })).failures[0]).toContain("invalid pack");
  });
});

describe("permission matching used by forge test", () => {
  test.each([
    ["Bash(git clean:*)", "Bash(git clean -fd)", true],
    ["Bash(git clean:*)", "Bash(git clean)", true],
    ["Bash(git clean:*)", "Bash(git cleanup-branches)", false],
    ["Read(**/.env)", "Read(apps/api/.env)", true],
    ["Read(**/.env)", "Read(.env.example)", false],
    ["Read(**/.env)", "Edit(.env)", false],
  ])("%s matches %s: %p", (rule, call, want) => {
    expect(permissionMatches(rule, call)).toBe(want);
  });
});
