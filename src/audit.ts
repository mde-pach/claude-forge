import { existsSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { basename, dirname, join } from "node:path";
import { run } from "./util.js";
import type { Rule, ToolTarget } from "./types.js";

export interface AuditResult {
  rule: string;
  tool: ToolTarget["tool"];
  expects: string;
  status: "satisfied" | "unsatisfied" | "skipped";
  /** Why unsatisfied / skipped. */
  detail?: string;
  snippet?: string;
}

/** The project's own binary for a tool, or undefined. */
export function projectBinary(repo: string, tool: string): string | undefined {
  for (const p of [join(repo, "node_modules", ".bin", tool), join(repo, ".venv", "bin", tool)]) if (existsSync(p)) return p;
  return undefined;
}

const EXT: Record<Rule["scope"]["language"], string> = { python: ".py", typescript: ".ts", tsx: ".tsx", any: ".txt" };

/** A concrete relative path inside the rule's scope for an example file. */
export function examplePath(rule: Rule, stem: string): string {
  const glob = (rule.scope.files ?? []).find((g) => !g.startsWith("!"));
  const ext = EXT[rule.scope.language];
  if (!glob) return stem + ext;
  let p = glob
    .replace(/\{([^,}]*)[^}]*\}/g, "$1")
    .replace(/\.\*$/, ext)
    .replace(/(^|\/)\*\*\//g, "$1")
    .replace(/\*\*/g, stem)
    .replace(/\*/g, stem)
    .replace(/\?/g, "x");
  if (p.endsWith("/")) p += stem;
  if (!/\.[A-Za-z0-9]+$/.test(basename(p))) p += ext;
  return p;
}

/** How the tool reported the expected diagnostic on one example. */
export type Verdict =
  /** reported, and it fails the tool's CI-style run */
  | "fails"
  /** reported, but at a level that does not fail the run (a warning) */
  | "warning"
  /** not reported */
  | "absent";

interface ToolRun {
  status: number;
  stdout: string;
  stderr: string;
}

/** Runs the project's tool on one example with machine-readable output where the tool has it. */
function runTool(repo: string, rule: Rule, t: ToolTarget, bin: string, code: string, n: number): ToolRun {
  const stem = `forge_audit_${process.pid}_${n}`;
  const rel = examplePath(rule, stem);
  if (t.tool === "ruff") return run(bin, ["check", "--no-cache", "--no-fix", "--output-format", "json", "--stdin-filename", rel, "-"], { cwd: repo, input: code });
  // biome (its stdin mode prints no lint diagnostics), basedpyright and tsc
  // read real files: a temporary file inside the project.
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
    write(rel, code);
    if (t.tool === "biome") return run(bin, ["lint", "--reporter=json", "--max-diagnostics=50", rel], { cwd: repo });
    if (t.tool === "basedpyright") return run(bin, ["--outputjson", rel], { cwd: repo });
    const cfg = `tsconfig.${stem}.json`;
    write(
      cfg,
      JSON.stringify({ extends: "./tsconfig.json", compilerOptions: { noEmit: true, incremental: false, composite: false }, include: [], files: [rel] }),
    );
    return run(bin, ["--noEmit", "-p", cfg, "--pretty", "false"], { cwd: repo });
  } finally {
    for (const p of created) rmSync(p, { recursive: true, force: true });
  }
}

function parseJson(text: string): unknown {
  const start = text.search(/[[{]/);
  if (start < 0) throw new Error("no JSON in the output");
  return JSON.parse(text.slice(start));
}

/**
 * Whether the run reported `expects`, and whether that fails the run: an error
 * does; a warning does only when the run exits non-zero with no error
 * diagnostic at all (the tool fails on warnings, e.g. basedpyright's
 * failOnWarnings). Throws when the output cannot be read.
 */
export function verdict(tool: ToolTarget["tool"], expects: string, r: ToolRun): Verdict {
  const judge = (all: { id: string; severity: string }[]): Verdict => {
    const mine = all.filter((d) => d.id === expects);
    if (!mine.length) return "absent";
    if (mine.some((d) => d.severity === "error")) return "fails";
    const warningsFail = r.status !== 0 && !all.some((d) => d.severity === "error");
    return warningsFail ? "fails" : "warning";
  };
  if (tool === "tsc") return new RegExp(`\\berror ${expects}\\b`).test(r.stdout + r.stderr) ? "fails" : "absent";
  if (tool === "ruff") {
    // ruff has no warning level: every reported diagnostic fails `ruff check`
    const items = parseJson(r.stdout) as { code: string | null }[];
    return items.some((d) => d.code === expects) ? "fails" : "absent";
  }
  if (tool === "biome") {
    const out = parseJson(r.stdout) as { diagnostics?: { category?: string; severity?: string }[] };
    return judge((out.diagnostics ?? []).map((d) => ({ id: d.category ?? "", severity: d.severity ?? "" })));
  }
  const out = parseJson(r.stdout) as { generalDiagnostics?: { rule?: string; severity?: string }[] };
  return judge((out.generalDiagnostics ?? []).map((d) => ({ id: d.rule ?? "", severity: d.severity ?? "" })));
}

/** §11: every match example fails the tool's run on the expected diagnostic; no no_match example reports it. */
export function auditTarget(repo: string, rule: Rule, t: ToolTarget): AuditResult {
  const base = { rule: rule.id, tool: t.tool, expects: t.expects };
  const bin = projectBinary(repo, t.tool);
  if (!bin) return { ...base, status: "skipped", detail: `${t.tool} is configured but not installed (node_modules/.bin or .venv/bin)` };
  const match = rule.examples?.match ?? [];
  const noMatch = rule.examples?.no_match ?? [];
  if (!match.length) return { ...base, status: "skipped", detail: "the rule has no match examples" };
  const judge = (code: string, n: number): Verdict | AuditResult => {
    const r = runTool(repo, rule, t, bin, code, n);
    try {
      return verdict(t.tool, t.expects, r);
    } catch {
      const said = (r.stdout + r.stderr).split("\n").map((l) => l.trim()).find((l) => l && !/^[━─-]+$/.test(l));
      return { ...base, status: "skipped", detail: `could not read ${t.tool}'s output${said ? `: ${firstLine(said)}` : ""}` };
    }
  };
  let n = 0;
  for (const code of match) {
    const v = judge(code, n++);
    if (typeof v !== "string") return v;
    if (v === "absent") return { ...base, status: "unsatisfied", detail: `${t.expects} not reported for: ${firstLine(code)}`, snippet: t.snippet };
    if (v === "warning")
      return {
        ...base,
        status: "unsatisfied",
        detail: `${t.expects} is only a warning in your ${t.tool} config, so ${t.tool} does not fail on it (e.g. ${firstLine(code)})`,
        snippet: t.snippet,
      };
  }
  for (const code of noMatch) {
    const v = judge(code, n++);
    if (typeof v !== "string") return v;
    if (v !== "absent") return { ...base, status: "unsatisfied", detail: `${t.expects} reported for a no_match example: ${firstLine(code)}`, snippet: t.snippet };
  }
  return { ...base, status: "satisfied" };
}

function firstLine(s: string): string {
  const l = s.split("\n")[0] ?? "";
  return l.length > 60 ? l.slice(0, 57) + "..." : l;
}

export function auditRules(repo: string, items: { rule: Rule; audits: ToolTarget[] }[]): AuditResult[] {
  return items.flatMap(({ rule, audits }) => audits.map((t) => auditTarget(repo, rule, t)));
}
