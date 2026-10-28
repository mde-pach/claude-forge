import { existsSync } from "node:fs";
import { loadLocalRules, loadPacks } from "../packs.js";
import { applyRoles, defaultRoles } from "../roles.js";
import { kindOf, TARGET_KINDS, TARGETS } from "../targets/index.js";
import type { TargetKind } from "../targets/kind.js";
import type { ExampleFixture, Rule, TestReport } from "../types.js";
import { defaultToolBins } from "./runners.js";

export * from "./runners.js";
export type { TestReport };

/** The counts line of a test run, after `what` (packs and rules, or local rules). */
export function testSummary(c: Record<string, number>, what: string): string {
  return `${what}; ast-grep ${c.astgrep}, guard ${c.guard}, permission ${c.permission}, grep ${c.grep}, check ${c.check}, context ${c.context}`;
}

/**
 * The development self-test (`bun run test:packs`, the spec suite): every pack and rule
 * of packsDir (and, with repo, its local rules replacing or adding to them) validated
 * and their examples run.
 */
export function selfTest(packsDir: string, repo?: string, opts: { toolBins?: string[] } = {}): TestReport {
  const failures: string[] = [];
  const notes: string[] = [];
  const counts: Record<string, number> = { packs: 0, rules: 0, astgrep: 0, guard: 0, permission: 0, grep: 0, check: 0, context: 0 };

  const rules: Rule[] = [];
  try {
    const packs = loadPacks(packsDir);
    counts.packs = packs.size;
    for (const p of packs.values()) {
      rules.push(...p.rules);
      if (p.context !== undefined) {
        counts.context!++;
        const lines = p.context.replace(/\n$/, "").split("\n").length;
        if (lines > 10) failures.push(`${p.id}/context.md: ${lines} lines (max 10)`);
      }
    }
    if (repo) {
      const local = loadLocalRules(repo);
      const ids = new Map(rules.map((r, i) => [r.id, i]));
      for (const r of local) {
        const i = ids.get(r.id);
        if (i !== undefined) rules[i] = r;
        else rules.push(r);
      }
      if (local.length) notes.push(`including ${local.length} local rule(s) from .forge/rules`);
    }
  } catch (e) {
    failures.push((e as Error).message);
    return { failures, notes, counts };
  }
  testRules(rules, repo, opts, { failures, notes, counts });
  if (!existsSync(packsDir)) failures.push(`${packsDir} does not exist`);
  return { failures, notes, counts };
}

/**
 * `forge test` in a project: the rules of .forge/rules only (a local rule replacing a
 * pack rule is tested as written), validated and their examples run. The bundled
 * packs are proved by Forge's own test suite.
 */
export function localSelfTest(repo: string, opts: { toolBins?: string[] } = {}): TestReport {
  const failures: string[] = [];
  const notes: string[] = [];
  const counts: Record<string, number> = { packs: 0, rules: 0, astgrep: 0, guard: 0, permission: 0, grep: 0, check: 0, context: 0 };
  let rules: Rule[];
  try {
    rules = loadLocalRules(repo);
  } catch (e) {
    failures.push((e as Error).message);
    return { failures, notes, counts };
  }
  testRules(rules, repo, opts, { failures, notes, counts });
  return { failures, notes, counts };
}

/** Structural checks and every example of the given rules: what every rule needs, then each target kind's own self-test. */
function testRules(written: Rule[], repo: string | undefined, opts: { toolBins?: string[] }, report: TestReport): void {
  const { failures, counts } = report;
  // file roles (§15.4) with Forge's defaults: examples are written where a default project has them
  const rules = written.map((r) => {
    const { rule, empty } = applyRoles(r, defaultRoles());
    if (empty.length) failures.push(`${r.id}: file role ${empty.map((e) => `@${e}`).join(", ")} has no default files: its examples cannot be placed`);
    return rule;
  });
  counts.rules = rules.length;

  // structural checks the schema cannot express
  for (const r of rules) {
    if (r.enforce.some((t) => kindOf(t).needsExamples(t)) && !(r.examples?.match?.length && r.examples?.no_match?.length)) failures.push(`${r.id}: needs match and no_match examples`);
    const all = [...(r.examples?.match ?? []), ...(r.examples?.no_match ?? [])];
    // fixtures (files): check rules; a rule of tool targets only may give one file without head (its path picks the file type)
    const fixtures = all.filter((e): e is ExampleFixture => typeof e !== "string" && !("command" in e));
    const toolOnly = r.enforce.every((t) => t.target === "tool");
    if (fixtures.length && !r.enforce.every((t) => t.target === "check") && !(toolOnly && fixtures.every((f) => !f.head && Object.keys(f.files).length === 1)))
      failures.push(`${r.id}: fixture examples (files) are for check rules only, or one file without head for a rule of tool targets`);
    if (all.some((e) => typeof e !== "string" && "command" in e) && r.enforce.some((t) => t.target !== "guard")) failures.push(`${r.id}: command examples are for guard rules only`);
    // examples.deps install what the types engine type-checks the examples against (§15.1)
    if (r.examples?.deps && !r.enforce.some((t) => t.target === "ast-grep" && t.where)) failures.push(`${r.id}: examples.deps is for rules with a where`);
  }

  // each kind's examples, with the rules that have such a target
  const ctx = { report, repo, toolBins: opts.toolBins ?? defaultToolBins(repo) };
  for (const k of TARGET_KINDS) {
    const kind = TARGETS[k] as TargetKind;
    const mine = rules.filter((r) => r.enforce.some((t) => t.target === k));
    if (kind.selftest && mine.length) kind.selftest(mine, ctx);
  }
  counts.types ??= 0;
}
