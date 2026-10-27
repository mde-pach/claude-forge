import { defaultRoles, expandGlobs } from "./roles.js";
import { existsSync, mkdirSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { basename, dirname, join, posix } from "node:path";
import { toolExamples } from "./packs.js";
import { ENGINE_PYTHON, run, TY_VERSION } from "./util.js";
import type { Rule, ToolTarget } from "./types.js";

export interface AuditResult {
  rule: string;
  tool: ToolTarget["tool"];
  expects: string;
  status: "satisfied" | "unsatisfied" | "skipped";
  /** The workspace package audited (its own config); absent: the root. */
  dir?: string;
  /** Why unsatisfied / skipped. */
  detail?: string;
  snippet?: string;
}

/** Forge's pinned ty (§15.2), run through uvx: the argv before ty's own arguments. */
export const TY_COMMAND = ["uvx", "-q", "--python", ENGINE_PYTHON, "--from", `ty==${TY_VERSION}`, "ty"];

/** The project's own binary for a tool, or undefined. */
export function projectBinary(repo: string, tool: string): string | undefined {
  for (const p of [join(repo, "node_modules", ".bin", tool), join(repo, ".venv", "bin", tool)]) if (existsSync(p)) return p;
  return undefined;
}

const EXT: Record<Rule["scope"]["language"], string> = { python: ".py", typescript: ".ts", tsx: ".tsx", yaml: ".yaml", any: ".txt" };

/** A concrete relative path inside the rule's scope for an example file. */
export function examplePath(rule: Rule, stem: string): string {
  // a rule as written names file roles (§15.4): its examples go where Forge's defaults put them
  const glob = expandGlobs(rule.scope.files ?? [], defaultRoles()).globs.find((g) => !g.startsWith("!"));
  const ext = EXT[rule.scope.language];
  if (!glob) return stem + ext;
  let p = glob
    .replace(/\{([^,}]*)[^}]*\}/g, "$1")
    .replace(/\.\*$/, ext)
    .replace(/(^|\/)\*\*\//g, "$1")
    .replace(/\*\*/g, stem)
    .replace(/\*/g, stem)
    .replace(/\?/g, "x")
    .replace(/\[!?(.)[^\]]*\]/g, (m, c: string) => (m[1] === "!" ? "x" : c));
  if (p.endsWith("/")) p += stem;
  if (!/\.[A-Za-z0-9]+$/.test(basename(p))) p += ext;
  return p;
}

/** Where example n is written: in the rule's scope, or, for a one-file fixture, next to its path with its file type (stem.vue). */
function exampleRel(rule: Rule, ex: Example, n: number): string {
  if (!ex.path) return examplePath(rule, stemOf(n));
  const name = posix.basename(ex.path);
  const dir = posix.dirname(ex.path);
  const rel = stemOf(n) + (name.includes(".") ? name.slice(name.indexOf(".")) : "");
  return dir === "." ? rel : `${dir}/${rel}`;
}

type Example = { code: string; path?: string };

/** How the tool reported the expected diagnostic on one example. */
export type Verdict =
  /** reported as an error */
  | "fails"
  /** reported as a warning: the check stage does not block on it (§10.1), even when the tool exits non-zero */
  | "warning"
  /** not reported */
  | "absent";

interface ToolRun {
  status: number;
  stdout: string;
  stderr: string;
}

const stemOf = (n: number) => `forge_audit_${process.pid}_${n}`;

/**
 * Writes temporary files inside the project (relative path → text) and runs <fn>; the
 * files, and the directories made for them, are removed afterwards.
 */
function withFiles<T>(repo: string, files: [string, string][], fn: () => T): T {
  const created: string[] = [];
  const write = (p: string, text: string) => {
    const abs = join(repo, p);
    let d = dirname(abs);
    const missing: string[] = [];
    while (!existsSync(d)) {
      missing.push(d);
      d = dirname(d);
    }
    for (const m of missing.reverse()) {
      mkdirSync(m);
      created.unshift(m);
    }
    writeFileSync(abs, text);
    created.unshift(abs);
  };
  try {
    for (const [p, text] of files) write(p, text);
    return fn();
  } finally {
    for (const p of created) rmSync(p, { recursive: true, force: true });
  }
}

/** Tools that report by file: the examples of a target are checked in one run (batchVerdicts). */
const BY_FILE = new Set<ToolTarget["tool"]>(["biome", "ty", "eslint"]);

/** biome, eslint and ty on example files (relative paths), with machine-readable output. */
function runOnFiles(repo: string, t: ToolTarget, bin: string, rels: string[]): ToolRun {
  if (t.tool === "biome") return run(bin, ["lint", "--reporter=json", "--max-diagnostics=50", ...rels], { cwd: repo });
  if (t.tool === "eslint") return run(bin, ["--format", "json", "--no-warn-ignored", ...rels], { cwd: repo });
  return run(TY_COMMAND[0]!, [...TY_COMMAND.slice(1), "check", "--output-format", "gitlab", ...rels], { cwd: repo });
}

/** Runs the project's tool on one example with machine-readable output where the tool has it. */
function runTool(repo: string, rule: Rule, t: ToolTarget, bin: string, ex: Example, n: number): ToolRun {
  const stem = stemOf(n);
  const rel = exampleRel(rule, ex, n);
  const code = ex.code;
  if (t.tool === "ruff") return run(bin, ["check", "--no-cache", "--no-fix", "--output-format", "json", "--stdin-filename", rel, "-"], { cwd: repo, input: code });
  // biome (its stdin mode prints no lint diagnostics), ty and tsc read real files: a
  // temporary file inside the project.
  if (BY_FILE.has(t.tool)) return withFiles(repo, [[rel, code]], () => runOnFiles(repo, t, bin, [rel]));
  const cfg = `tsconfig.${stem}.json`;
  const tsconfig = JSON.stringify({ extends: "./tsconfig.json", compilerOptions: { noEmit: true, incremental: false, composite: false }, include: [], files: [rel] });
  return withFiles(repo, [[rel, code], [cfg, tsconfig]], () => run(bin, ["--noEmit", "-p", cfg, "--pretty", "false"], { cwd: repo }));
}

function parseJson(text: string): unknown {
  const start = text.search(/[[{]/);
  if (start < 0) throw new Error("no JSON in the output");
  return JSON.parse(text.slice(start));
}

/**
 * Whether the run reported `expects`, and at which level: only an error enforces the
 * rule, since the check stage ignores warnings (§10.1), also when the tool exits
 * non-zero on them (ty's terminal.error-on-warning). Throws when the output cannot be read.
 */
export function verdict(tool: ToolTarget["tool"], expects: string, r: ToolRun): Verdict {
  if (tool === "tsc") return new RegExp(`\\berror ${expects}\\b`).test(r.stdout + r.stderr) ? "fails" : "absent";
  if (tool === "ruff") {
    // ruff has no warning level: every reported diagnostic fails `ruff check`
    const items = parseJson(r.stdout) as { code: string | null }[];
    return items.some((d) => d.code === expects) ? "fails" : "absent";
  }
  return judge(expects, diagnostics(tool, r, "").list);
}

interface Diagnostic {
  id: string;
  severity: string;
  /** the file, as the tool names it */
  file: string;
}

/**
 * biome's, eslint's or ty's diagnostics; complete: none left out (biome's --max-diagnostics).
 * Throws when the output cannot be read, and on a file eslint could not parse (a fatal
 * message: nothing was linted, so nothing can be judged). eslint names files by absolute
 * path: made relative to repo.
 */
function diagnostics(tool: "biome" | "ty" | "eslint", r: ToolRun, repo: string): { list: Diagnostic[]; complete: boolean } {
  if (tool === "eslint") {
    const out = parseJson(r.stdout) as { filePath?: string; messages?: { ruleId?: string | null; severity?: number; fatal?: boolean; message?: string }[] }[];
    const roots = repo ? [repo, realpathSync(repo)] : [];
    const rel = (f = "") => {
      for (const root of roots) if (f.startsWith(`${root}/`)) return f.slice(root.length + 1);
      return f;
    };
    const fatal = out.flatMap((f) => f.messages ?? []).find((m) => m.fatal);
    if (fatal) throw new Error(`eslint could not parse the example: ${fatal.message ?? ""}`);
    return {
      list: out.flatMap((f) => (f.messages ?? []).map((m) => ({ id: m.ruleId ?? "", severity: m.severity === 2 ? "error" : m.severity === 1 ? "warning" : "", file: rel(f.filePath) }))),
      complete: true,
    };
  }
  if (tool === "biome") {
    const out = parseJson(r.stdout) as {
      summary?: { diagnosticsNotPrinted?: number };
      diagnostics?: { category?: string; severity?: string; location?: { path?: string | { file?: string } } }[];
    };
    const file = (p?: string | { file?: string }) => (typeof p === "string" ? p : (p?.file ?? ""));
    return {
      list: (out.diagnostics ?? []).map((d) => ({ id: d.category ?? "", severity: d.severity ?? "", file: file(d.location?.path) })),
      complete: out.summary?.diagnosticsNotPrinted === 0,
    };
  }
  // ty: GitLab Code Quality JSON; "major" is an error, "minor" a warning
  const out = parseJson(r.stdout) as { check_name?: string; severity?: string; location?: { path?: string } }[];
  const severity = (s?: string) => (s === "major" || s === "critical" || s === "blocker" ? "error" : s === "minor" ? "warning" : (s ?? ""));
  return { list: out.map((d) => ({ id: d.check_name ?? "", severity: severity(d.severity), file: d.location?.path ?? "" })), complete: true };
}

/** The verdict on the diagnostics of a run (all: of one example). */
function judge(expects: string, all: { id: string; severity: string }[]): Verdict {
  const mine = all.filter((d) => d.id === expects);
  if (!mine.length) return "absent";
  return mine.some((d) => d.severity === "error") ? "fails" : "warning";
}

/**
 * biome and ty report by file, so the examples of a target run in one process (several
 * when two examples have the same path). Per example, the verdict is the one a run on
 * that example alone gives: its own diagnostics. undefined for
 * an example the batch cannot judge (output not read, diagnostics left out, a file name
 * that is not one of the examples'): it runs alone.
 */
function batchVerdicts(repo: string, rule: Rule, t: ToolTarget, bin: string, examples: Example[]): (Verdict | undefined)[] {
  const out: (Verdict | undefined)[] = new Array(examples.length);
  const codes = examples.map((e) => e.code);
  const rels = examples.map((e, n) => exampleRel(rule, e, n));
  const batches: number[][] = [];
  rels.forEach((rel, n) => {
    const free = batches.find((b) => !b.some((m) => rels[m] === rel));
    if (free) free.push(n);
    else batches.push([n]);
  });
  for (const b of batches) {
    const r = withFiles(
      repo,
      b.map((n) => [rels[n]!, codes[n]!]),
      () => runOnFiles(repo, t, bin, b.map((n) => rels[n]!)),
    );
    let d: ReturnType<typeof diagnostics>;
    try {
      d = diagnostics(t.tool as "biome" | "ty" | "eslint", r, repo);
    } catch {
      continue;
    }
    const mine = new Set(b.map((n) => rels[n]!));
    if (!d.complete || d.list.some((x) => !mine.has(x.file))) continue;
    for (const n of b) out[n] = judge(t.expects, d.list.filter((x) => x.file === rels[n]));
  }
  return out;
}

const uvx = new Map<string, boolean>();
/** uvx is on PATH (looked up once per PATH). */
function hasUvx(): boolean {
  const path = process.env.PATH ?? "";
  if (!uvx.has(path)) uvx.set(path, run("sh", ["-c", "command -v uvx"]).status === 0);
  return uvx.get(path)!;
}

/** §11: every match example fails the tool's run on the expected diagnostic; no no_match example reports it. */
export function auditTarget(repo: string, rule: Rule, t: ToolTarget, root = repo): AuditResult {
  const base = { rule: rule.id, tool: t.tool, expects: t.expects };
  // in a workspace package, the tool may be installed at the root (hoisted)
  // ty is Forge's own (through uvx); the others are the project's
  const bin = t.tool === "ty" ? (hasUvx() ? "uvx" : undefined) : (projectBinary(repo, t.tool) ?? projectBinary(root, t.tool));
  if (!bin) return { ...base, status: "skipped", detail: t.tool === "ty" ? "uvx is not installed (Forge runs its pinned ty through it)" : `${t.tool} is configured but not installed (node_modules/.bin or .venv/bin)` };
  const match = toolExamples(rule, "match");
  const noMatch = toolExamples(rule, "no_match");
  if (!match.length) return { ...base, status: "skipped", detail: "the rule has no match examples" };
  const batched = BY_FILE.has(t.tool) ? batchVerdicts(repo, rule, t, bin, [...match, ...noMatch]) : [];
  const judge = (ex: Example, n: number): Verdict | AuditResult => {
    const v = batched[n];
    if (v) return v;
    const r = runTool(repo, rule, t, bin, ex, n);
    try {
      return verdict(t.tool, t.expects, r);
    } catch (e) {
      const said = /^eslint could not parse/.test((e as Error).message) ? (e as Error).message : (r.stdout + r.stderr).split("\n").map((l) => l.trim()).find((l) => l && !/^[━─-]+$/.test(l));
      return { ...base, status: "skipped", detail: `could not read ${t.tool}'s output${said ? `: ${firstLine(said)}` : ""}` };
    }
  };
  let n = 0;
  for (const ex of match) {
    const v = judge(ex, n++);
    const code = ex.path ? `${ex.path}: ${ex.code}` : ex.code;
    if (typeof v !== "string") return v;
    if (v === "absent") return { ...base, status: "unsatisfied", detail: `${t.expects} not reported for: ${firstLine(code)}`, snippet: t.snippet };
    if (v === "warning")
      return {
        ...base,
        status: "unsatisfied",
        detail: `${t.expects} is only a warning in your ${t.tool} config, and Forge's checks do not block on warnings (e.g. ${firstLine(code)})`,
        snippet: t.snippet,
      };
  }
  for (const ex of noMatch) {
    const v = judge(ex, n++);
    const code = ex.path ? `${ex.path}: ${ex.code}` : ex.code;
    if (typeof v !== "string") return v;
    if (v !== "absent") return { ...base, status: "unsatisfied", detail: `${t.expects} reported for a no_match example: ${firstLine(code)}`, snippet: t.snippet };
  }
  return { ...base, status: "satisfied" };
}

function firstLine(s: string): string {
  const l = s.split("\n")[0] ?? "";
  return l.length > 60 ? l.slice(0, 57) + "..." : l;
}

/** Audits every tool target; dirOf gives where a tool is configured (a workspace package, relative to repo). */
/** Each tool target audited in every place that configures the tool (the root "", or workspace packages, each with its own config). */
export function auditRules(repo: string, items: { rule: Rule; audits: ToolTarget[] }[], dirsOf: (tool: ToolTarget["tool"]) => string[] = () => [""]): AuditResult[] {
  return items.flatMap(({ rule, audits }) =>
    audits.flatMap((t) => (dirsOf(t.tool).length ? dirsOf(t.tool) : [""]).map((d) => ({ ...auditTarget(join(repo, d), rule, t, repo), ...(d ? { dir: d } : {}) }))),
  );
}
