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

/** Runs the project's tool on one example; returns the combined output, or undefined if it could not run. */
function runTool(repo: string, rule: Rule, t: ToolTarget, bin: string, code: string, n: number): string | undefined {
  const stem = `forge_audit_${process.pid}_${n}`;
  const rel = examplePath(rule, stem);
  if (t.tool === "ruff") {
    const r = run(bin, ["check", "--no-cache", "--no-fix", "--output-format", "concise", "--stdin-filename", rel, "-"], { cwd: repo, input: code });
    return r.stdout + r.stderr;
  }
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
    if (t.tool === "biome") {
      const r = run(bin, ["lint", "--max-diagnostics=50", rel], { cwd: repo });
      return r.stdout + r.stderr;
    }
    if (t.tool === "basedpyright") {
      const r = run(bin, [rel], { cwd: repo });
      return r.stdout + r.stderr;
    }
    const cfg = `tsconfig.${stem}.json`;
    write(
      cfg,
      JSON.stringify({ extends: "./tsconfig.json", compilerOptions: { noEmit: true, incremental: false, composite: false }, include: [], files: [rel] }),
    );
    const r = run(bin, ["--noEmit", "-p", cfg, "--pretty", "false"], { cwd: repo });
    return r.stdout + r.stderr;
  } finally {
    for (const p of created) rmSync(p, { recursive: true, force: true });
  }
}

/** §11: the expected diagnostic appears for every match example and for no no_match example. */
export function auditTarget(repo: string, rule: Rule, t: ToolTarget): AuditResult {
  const base = { rule: rule.id, tool: t.tool, expects: t.expects };
  const bin = projectBinary(repo, t.tool);
  if (!bin) return { ...base, status: "skipped", detail: `${t.tool} is configured but not installed (node_modules/.bin or .venv/bin)` };
  const match = rule.examples?.match ?? [];
  const noMatch = rule.examples?.no_match ?? [];
  if (!match.length) return { ...base, status: "skipped", detail: "the rule has no match examples" };
  const has = (out: string) => out.includes(t.expects);
  let n = 0;
  for (const code of match) {
    const out = runTool(repo, rule, t, bin, code, n++);
    if (out === undefined) return { ...base, status: "skipped", detail: `${t.tool} could not run` };
    if (!has(out)) {
      const said = out.split("\n").map((l) => l.trim()).find((l) => l && !/^[━─-]+$/.test(l));
      return {
        ...base,
        status: "unsatisfied",
        detail: `${t.expects} not reported for: ${firstLine(code)}${said ? `; ${t.tool} said: ${firstLine(said)}` : ""}`,
        snippet: t.snippet,
      };
    }
  }
  for (const code of noMatch) {
    const out = runTool(repo, rule, t, bin, code, n++);
    if (out !== undefined && has(out))
      return { ...base, status: "unsatisfied", detail: `${t.expects} reported for a no_match example: ${firstLine(code)}`, snippet: t.snippet };
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
