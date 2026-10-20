// Everything the site shows comes from here: packs/ (via Forge's own loader),
// tests/spec/ and the CI's JUnit results. Functions re-read on every call so
// the dev server is always live.
import { existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { marked } from "marked";
import { loadPacks } from "../../../src/packs";
import { checkTools } from "../../../src/render";
import type { Pack, Rule, Target } from "../../../src/types";
import { readResults, readSpec, summarize, type Results, type SpecNode, type SpecSection, type Status, type TestResult } from "./spec";

/** The repository root: the closest directory up from the working directory with packs/ and tests/spec/ (bundled pages cannot use import.meta.url). */
function findRoot(): string {
  for (let d = process.cwd(); ; d = dirname(d)) {
    if (existsSync(join(d, "packs")) && existsSync(join(d, "tests", "spec"))) return d;
    if (dirname(d) === d) throw new Error("forge docs: run from inside the repository");
  }
}
export const ROOT = findRoot();
export const REPO_URL = "https://github.com/mde-pach/claude-forge";

export const packs = (): Map<string, Pack> => loadPacks(join(ROOT, "packs"));
export const spec = (): SpecSection[] => readSpec(ROOT);
export const results = (): Results => readResults(ROOT);

/** A site URL under the configured base. */
export function url(path: string): string {
  const base = import.meta.env.BASE_URL.replace(/\/$/, "");
  return `${base}/${path.replace(/^\//, "")}`;
}

export const packUrl = (id: string) => url(`packs/${id}/`);
export const ruleAnchor = (id: string) => id.replaceAll("/", "-");

// ---------------------------------------------------------------- markdown

export function markdown(md: string): string {
  return marked.parse(md, { async: false });
}

/** Test titles: plain text with `code` spans. */
export function inline(text: string): string {
  const esc = text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  return esc.replace(/`([^`]+)`/g, "<code>$1</code>");
}

// ---------------------------------------------------------------- rule results

/** Results of "<rule-id>: examples behave as declared" in the packs spec. */
export function ruleResults(r: Results, ruleId: string): TestResult[] {
  const out: TestResult[] = [];
  for (const rs of r.byLocation.values()) for (const x of rs) if (x.name === `${ruleId}: examples behave as declared`) out.push(x);
  return out;
}

export function ruleStatus(r: Results, ruleId: string): Status | "not run" {
  return summarize(ruleResults(r, ruleId));
}

// ---------------------------------------------------------------- packs

export function lineage(all: Map<string, Pack>, id: string): Pack[] {
  const out: Pack[] = [];
  for (let p = all.get(id); p; p = p.parent ? all.get(p.parent) : undefined) out.unshift(p);
  return out;
}

export function leaf(id: string): string {
  return id.split("/").pop()!;
}

export function targetLabel(t: Target): string {
  switch (t.target) {
    case "tool":
      return `${t.tool} audit`;
    case "ast-grep":
      return `ast-grep ${t.mode}`;
    case "permission":
      return `permission ${t.decision}`;
    case "guard":
      return `guard ${t.decision}`;
    case "sandbox":
      return "sandbox";
    case "check":
      return `check · ${t.stage}`;
  }
}

export function enforcedBy(rule: Rule): string[] {
  return [...new Set(rule.enforce.map(targetLabel))];
}

export { checkTools };
export type { Pack, Rule, Target, Status, TestResult, SpecNode, SpecSection, Results };

/** Fails the build when a CI result cannot be placed on a test in tests/spec (the docs would be lying by omission). */
export function assertResultsMatch(sections: SpecSection[], r: Results): void {
  const known = new Set<string>();
  const visit = (file: string, nodes: SpecNode[]) => nodes.forEach((n) => (n.kind === "test" ? known.add(`${file}:${n.line}`) : visit(file, n.children)));
  sections.forEach((s) => visit(s.file, s.children));
  const unmatched = [...r.byLocation.keys()].filter((k) => !known.has(k));
  if (unmatched.length) throw new Error(`forge docs: test results without a matching test in tests/spec: ${unmatched.slice(0, 10).join(", ")}`);
}

// ---------------------------------------------------------------- tests on rule and pack pages

export interface PlacedTest {
  file: string;
  node: SpecNode;
  /** Only this case of a parameterised test. */
  instance?: string;
}

function findTests(sections: SpecSection[], keep: (t: Extract<SpecNode, { kind: "test" }>) => boolean): { file: string; node: Extract<SpecNode, { kind: "test" }> }[] {
  const out: { file: string; node: Extract<SpecNode, { kind: "test" }> }[] = [];
  const visit = (file: string, nodes: SpecNode[]) => nodes.forEach((n) => (n.kind === "test" ? keep(n) && out.push({ file, node: n }) : visit(file, n.children)));
  sections.forEach((s) => visit(s.file, s.children));
  return out;
}

/** The rule's example test (its own case), then every test tagged @rule <id>. */
export function ruleTests(sections: SpecSection[], ruleId: string): PlacedTest[] {
  return [
    ...findTests(sections, (t) => t.title === "%s: examples behave as declared").map((t) => ({ ...t, instance: `${ruleId}: ` })),
    ...findTests(sections, (t) => t.rules.includes(ruleId)),
  ];
}

/** The pack's context test (its own case), then every test tagged @pack <id>. */
export function packTests(sections: SpecSection[], pack: Pack): PlacedTest[] {
  return [
    ...(pack.context !== undefined ? findTests(sections, (t) => t.title === "%s: context.md is at most 10 lines").map((t) => ({ ...t, instance: `${pack.id}: ` })) : []),
    ...findTests(sections, (t) => t.packs.includes(pack.id)),
  ];
}
