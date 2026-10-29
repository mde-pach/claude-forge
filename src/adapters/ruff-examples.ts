/**
 * A `tool: ruff` target's match examples as a project of their own, with the rule's config
 * (its snippet) selecting exactly <code>: how scripts/ruff-rules.ts finds from which release
 * a code reports its examples, and how the adapter tests prove it on real binaries (§16.4).
 */
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { parse, stringify } from "smol-toml";
import { toolExamples } from "../examples.js";
import type { Rule, ToolTarget } from "../types.js";

type Toml = Record<string, unknown>;

/** The rule's snippet as a pyproject.toml selecting only <code> (the code the release gives the rule), for Python 3.12. */
export function ruffExampleConfig(t: ToolTarget, code: string): string {
  const snippet = parse(t.snippet.replaceAll(t.expects, code).replace(/^(\s*\[[^\]]+\])[ \t]+(?=[^\s#])/gm, "$1\n")) as Toml;
  const tool = ((snippet.tool as Toml | undefined) ?? {}) as Toml;
  const ruff = ((tool.ruff as Toml | undefined) ?? {}) as Toml;
  const lint = { ...((ruff.lint as Toml | undefined) ?? {}) };
  delete lint["extend-select"];
  delete lint.select;
  delete lint["per-file-ignores"];
  return stringify({ project: { name: "examples", version: "0" }, tool: { ruff: { ...ruff, "target-version": "py312", lint: { ...lint, select: [code] } } } });
}

/** The rule's match examples that are Python files, in a new directory with ruffExampleConfig; their paths. */
export function ruffExampleProject(rule: Rule, t: ToolTarget, code: string): { dir: string; files: string[] } {
  const dir = mkdtempSync(join(tmpdir(), "forge-ruff-examples-"));
  writeFileSync(join(dir, "pyproject.toml"), ruffExampleConfig(t, code));
  const files: string[] = [];
  toolExamples(rule, "match").forEach((ex, i) => {
    // a Python file: a named one, or the code of a Python rule (pyproject.toml's RUF200 is linted from the project's config, not here)
    const name = ex.path ? (ex.path.endsWith(".py") && !ex.path.includes("/") ? ex.path : "") : rule.scope.language === "python" ? `example_${i + 1}.py` : "";
    if (!name) return;
    writeFileSync(join(dir, name), ex.code);
    files.push(name);
  });
  return { dir, files };
}

/** Whether ruff's JSON output reports <code> in every one of <files>. */
export function reportsEveryFile(json: string, code: string, files: string[]): boolean {
  let items: { code: string | null; filename: string }[];
  try {
    items = JSON.parse(json);
  } catch {
    return false;
  }
  return files.every((f) => items.some((d) => d.code === code && (d.filename === f || d.filename.endsWith(`/${f}`))));
}
