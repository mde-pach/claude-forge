// The executable specification: tests/spec/*.test.ts read with the TypeScript
// compiler API, joined with JUnit results (test-results/<platform>.xml) by
// file and line. The docs render only this; nothing is restated by hand.
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join, relative } from "node:path";
import ts from "typescript";

/** `@rule <id…>` / `@pack <id…>` tags in a doc comment: where else the test is shown. Inherited from enclosing describes. */
export interface Tags {
  rules: string[];
  packs: string[];
}

export interface SpecTest extends Tags {
  kind: "test";
  title: string;
  comment?: string;
  /** 1-based line of the test call, as JUnit reports it. */
  line: number;
  /** The test body, dedented. */
  code: string;
  /** Source of the `.each` table, when parameterised. */
  table?: string;
  condition?: string;
  skip?: boolean;
}

export interface SpecGroup extends Tags {
  kind: "describe";
  title: string;
  comment?: string;
  line: number;
  table?: string;
  condition?: string;
  children: SpecNode[];
}

export type SpecNode = SpecTest | SpecGroup;

export interface SpecSection {
  /** Repository-relative path of the test file. */
  file: string;
  slug: string;
  /** Spec section number, e.g. "10" or "14.2". */
  number: string;
  title: string;
  /** Markdown from the file's @spec comment. */
  intro: string;
  children: SpecNode[];
}

export type Status = "passed" | "failed" | "skipped";

export interface TestResult {
  platform: string;
  name: string;
  classname: string;
  status: Status;
  time: number;
  message?: string;
}

export interface RunMeta {
  sha?: string;
  runUrl?: string;
  date?: string;
}

// ---------------------------------------------------------------- extraction

const TEST = new Set(["test", "it"]);
const DESCRIBE = new Set(["describe"]);

function jsdoc(source: string, node: ts.Node): { comment?: string } & Tags {
  const ranges = ts.getLeadingCommentRanges(source, node.getFullStart()) ?? [];
  const tags: Tags = { rules: [], packs: [] };
  const docs = ranges
    .map((r) => source.slice(r.pos, r.end))
    .filter((c) => c.startsWith("/**"))
    .map((c) =>
      c
        .replace(/^\/\*\*\s?/, "")
        .replace(/\s*\*\/$/, "")
        .split("\n")
        .map((l) => l.replace(/^\s*\* ?/, ""))
        .filter((l) => {
          const m = /^@(rule|pack)\s+(.+)$/.exec(l.trim());
          if (m) (m[1] === "rule" ? tags.rules : tags.packs).push(...m[2]!.split(/[\s,]+/).filter(Boolean));
          return !m;
        })
        .join("\n")
        .trim(),
    )
    .filter(Boolean);
  return { comment: docs.length ? docs.join("\n\n") : undefined, ...tags };
}

function dedent(text: string): string {
  const lines = text.replace(/^\n+|\s+$/g, "").split("\n");
  const indent = Math.min(...lines.filter((l) => l.trim()).map((l) => /^ */.exec(l)![0].length));
  return lines.map((l) => l.slice(indent)).join("\n");
}

/** Describes a test/describe call: test(...), test.skip(...), test.each(t)(...), test.if(c)(...). */
function classify(call: ts.CallExpression, sf: ts.SourceFile) {
  let callee: ts.Expression = call.expression;
  let table: string | undefined;
  let condition: string | undefined;
  let skip = false;
  if (ts.isCallExpression(callee) && ts.isPropertyAccessExpression(callee.expression)) {
    const mod = callee.expression.name.text;
    const arg = callee.arguments[0]?.getText(sf);
    if (mod === "each") table = arg;
    else if (mod === "if") condition = arg;
    else if (mod === "skipIf") condition = `!(${arg})`;
    callee = callee.expression.expression;
  } else if (ts.isPropertyAccessExpression(callee)) {
    if (["skip", "todo"].includes(callee.name.text)) skip = true;
    callee = callee.expression;
  }
  if (!ts.isIdentifier(callee)) return undefined;
  const kind = TEST.has(callee.text) ? "test" : DESCRIBE.has(callee.text) ? "describe" : undefined;
  if (!kind) return undefined;
  const titleArg = call.arguments[0];
  if (!titleArg) return undefined;
  const title = ts.isStringLiteralLike(titleArg) ? titleArg.text : titleArg.getText(sf);
  return { kind, title, table, condition, skip, fn: call.arguments[1] } as const;
}

function walk(statements: ts.NodeArray<ts.Statement>, sf: ts.SourceFile, inherited: Tags = { rules: [], packs: [] }): SpecNode[] {
  const out: SpecNode[] = [];
  for (const st of statements) {
    if (!ts.isExpressionStatement(st) || !ts.isCallExpression(st.expression)) continue;
    const c = classify(st.expression, sf);
    if (!c) continue;
    // bun reports the line of the title argument
    const line = sf.getLineAndCharacterOfPosition(st.expression.arguments[0]!.getStart(sf)).line + 1;
    const doc = jsdoc(sf.text, st);
    const comment = doc.comment;
    const tags: Tags = { rules: [...inherited.rules, ...doc.rules], packs: [...inherited.packs, ...doc.packs] };
    const fn = c.fn && (ts.isArrowFunction(c.fn) || ts.isFunctionExpression(c.fn)) ? c.fn : undefined;
    if (c.kind === "describe") {
      const body = fn && ts.isBlock(fn.body) ? fn.body.statements : undefined;
      out.push({ kind: "describe", title: c.title, comment, line, table: c.table, condition: c.condition, ...tags, children: body ? walk(body, sf, tags) : [] });
    } else {
      let code = "";
      if (fn) code = ts.isBlock(fn.body) ? dedent(sf.text.slice(fn.body.getStart(sf) + 1, fn.body.getEnd() - 1)) : fn.body.getText(sf);
      out.push({ kind: "test", title: c.title, comment, line, code, table: c.table, condition: c.condition, skip: c.skip || undefined, ...tags });
    }
  }
  return out;
}

export function readSpecFile(path: string, root: string): SpecSection {
  const text = readFileSync(path, "utf8");
  const sf = ts.createSourceFile(path, text, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
  const header = /\/\*\*([\s\S]*?)\*\//.exec(text)?.[1] ?? "";
  const lines = header.split("\n").map((l) => l.replace(/^\s*\* ?/, ""));
  const tag = lines.findIndex((l) => l.startsWith("@spec "));
  if (tag < 0) throw new Error(`${path}: missing @spec header`);
  const m = /^@spec\s+([\d.]+)\s*·\s*(.+)$/.exec(lines[tag]!.trim());
  if (!m) throw new Error(`${path}: @spec header must be "@spec <number> · <title>"`);
  const file = relative(root, path);
  return {
    file,
    slug: file.split("/").pop()!.replace(/\.test\.ts$/, ""),
    number: m[1]!,
    title: m[2]!,
    intro: lines.slice(tag + 1).join("\n").trim(),
    children: walk(sf.statements, sf),
  };
}

export function readSpec(root: string): SpecSection[] {
  const dir = join(root, "tests", "spec");
  return readdirSync(dir)
    .filter((f) => f.endsWith(".test.ts"))
    .sort()
    .map((f) => readSpecFile(join(dir, f), root));
}

export function countTests(nodes: SpecNode[]): number {
  return nodes.reduce((n, x) => n + (x.kind === "test" ? 1 : countTests(x.children)), 0);
}

// ---------------------------------------------------------------- results

function attrs(tag: string): Record<string, string> {
  const out: Record<string, string> = {};
  for (const m of tag.matchAll(/([\w:-]+)="([^"]*)"/g)) out[m[1]!] = unescapeXml(m[2]!);
  return out;
}
function unescapeXml(s: string): string {
  return s.replace(/&(lt|gt|quot|apos|amp|#(\d+)|#x([0-9a-f]+));/gi, (_, e, d, h) =>
    d ? String.fromCodePoint(+d) : h ? String.fromCodePoint(parseInt(h, 16)) : ({ lt: "<", gt: ">", quot: '"', apos: "'", amp: "&" } as Record<string, string>)[e]!,
  );
}

/** JUnit testcases of one report, keyed by "file:line". */
export function parseJUnit(xml: string, platform: string): Map<string, TestResult[]> {
  const out = new Map<string, TestResult[]>();
  for (const m of xml.matchAll(/<testcase\b([^>]*?)(\/>|>([\s\S]*?)<\/testcase>)/g)) {
    const a = attrs(m[1]!);
    const body = m[3] ?? "";
    const failure = /<(failure|error)\b([^>]*)>/.exec(body);
    const status: Status = failure ? "failed" : /<skipped\b/.test(body) ? "skipped" : "passed";
    const key = `${a.file?.replace(/^\.\//, "")}:${a.line}`;
    const r: TestResult = { platform, name: a.name ?? "", classname: a.classname ?? "", status, time: Number(a.time ?? 0) };
    if (failure) r.message = attrs(failure[2]!).message;
    out.set(key, [...(out.get(key) ?? []), r]);
  }
  return out;
}

export interface Results {
  byLocation: Map<string, TestResult[]>;
  platforms: string[];
  meta: RunMeta;
}

/** Every test-results/<platform>.xml, plus test-results/meta.json from CI. */
export function readResults(root: string): Results {
  const dir = join(root, "test-results");
  const byLocation = new Map<string, TestResult[]>();
  const platforms: string[] = [];
  let meta: RunMeta = {};
  if (existsSync(dir)) {
    for (const f of readdirSync(dir).sort()) {
      if (f === "meta.json") meta = JSON.parse(readFileSync(join(dir, f), "utf8")) as RunMeta;
      if (!f.endsWith(".xml")) continue;
      const platform = f.slice(0, -4);
      platforms.push(platform);
      for (const [k, v] of parseJUnit(readFileSync(join(dir, f), "utf8"), platform)) byLocation.set(k, [...(byLocation.get(k) ?? []), ...v]);
    }
  }
  return { byLocation, platforms, meta };
}

export function resultsFor(results: Results, file: string, line: number): TestResult[] {
  return results.byLocation.get(`${file}:${line}`) ?? [];
}

/** passed only if every instance on every platform passed; "not run" without results. */
export function summarize(rs: TestResult[]): Status | "not run" {
  if (!rs.length) return "not run";
  if (rs.some((r) => r.status === "failed")) return "failed";
  if (rs.every((r) => r.status === "skipped")) return "skipped";
  return "passed";
}

export function tally(section: SpecNode[], file: string, results: Results): Record<Status | "not run", number> {
  const t = { passed: 0, failed: 0, skipped: 0, "not run": 0 };
  const visit = (nodes: SpecNode[]) => {
    for (const n of nodes) {
      if (n.kind === "describe") visit(n.children);
      else t[summarize(resultsFor(results, file, n.line))]++;
    }
  };
  visit(section);
  return t;
}

// ---------------------------------------------------------------- placement

/** Every test of a section, with the titles of its enclosing describes. */
export function allTests(nodes: SpecNode[], path: string[] = []): { test: SpecTest; path: string[] }[] {
  return nodes.flatMap((n) => (n.kind === "test" ? [{ test: n, path }] : allTests(n.children, [...path, n.title])));
}

/** A `<Spec file="…" describe="…" test="…" top />` usage in a docs page. */
export interface SpecUse {
  page: string;
  file: string;
  describe?: string;
  test?: string;
  top?: boolean;
}

/** Selects the nodes a <Spec> usage shows. */
export function selectNodes(section: SpecSection, use: Omit<SpecUse, "page" | "file">): SpecNode[] {
  if (use.test !== undefined) return allTests(section.children).filter((t) => t.test.title === use.test).map((t) => t.test);
  if (use.describe !== undefined) return section.children.filter((n) => n.kind === "describe" && n.title === use.describe);
  if (use.top) return section.children.filter((n) => n.kind === "test");
  return section.children;
}

/** <Spec> usages in docs/src (pages, components, content). */
export function specUses(root: string): SpecUse[] {
  const out: SpecUse[] = [];
  const visit = (dir: string) => {
    for (const f of readdirSync(dir, { withFileTypes: true })) {
      const p = join(dir, f.name);
      if (f.isDirectory()) visit(p);
      else if (/\.(mdx|astro)$/.test(f.name))
        for (const m of readFileSync(p, "utf8").matchAll(/<Spec\s([^>]*?)\/>/g)) {
          const a = (k: string) => new RegExp(`\\b${k}="([^"]*)"`).exec(m[1]!)?.[1];
          const file = a("file");
          if (file) out.push({ page: relative(root, p), file, describe: a("describe"), test: a("test"), top: /\btop\b/.test(m[1]!) || undefined });
        }
    }
  };
  visit(join(root, "docs", "src"));
  return out;
}

/** Tests shown on a rule or pack page automatically: its example test, its context test, and tagged tests. */
export function autoPlaced(t: SpecTest): boolean {
  return t.rules.length > 0 || t.packs.length > 0 || /: (examples behave as declared|context\.md is at most 10 lines)$/.test(t.title);
}
