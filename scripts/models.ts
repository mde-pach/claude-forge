// The ai-models data file (packs/ai-models/models.tsv) rendered into the rules that
// need it. A rule line that follows a "# models.tsv: <kind>" comment holds the
// rendered value; `bun scripts/models.ts` rewrites those lines after an edit of the
// data file, and tests/spec/13-packs.test.ts fails while a rule differs from it.
//
// Kinds:
//   retired                 grep ERE: a quoted retired id (pydantic-ai / LiteLLM prefixes allowed)
//   rejects <feature>       ast-grep regex: a string literal naming a model that rejects <feature>
//   advise                  the check command: forge_advise forge_model_ids <deprecated> <known>
import { readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";

export interface ModelRow {
  id: string;
  provider: string;
  status: "active" | "deprecated" | "retired";
  retires: string;
  replacement: string;
  rejects: string[];
}

export const MODELS_FILE = join(import.meta.dir, "..", "packs", "ai-models", "models.tsv");

export function loadModels(file = MODELS_FILE): ModelRow[] {
  return readFileSync(file, "utf8")
    .split("\n")
    .filter((l) => l.trim() && !l.startsWith("#"))
    .map((l, i) => {
      const [id, provider, status, retires, replacement, rejects] = l.split("\t");
      if (!id || !provider || !retires || !replacement || !rejects || !["active", "deprecated", "retired"].includes(status ?? ""))
        throw new Error(`${file}: row ${i + 1} must be id<TAB>provider<TAB>active|deprecated|retired<TAB>retires<TAB>replacement<TAB>rejects`);
      if (status === "deprecated" && replacement === "-") throw new Error(`${file}: deprecated ${id} names no replacement`);
      return { id, provider, status: status as ModelRow["status"], retires, replacement, rejects: rejects === "-" ? [] : rejects.split(",") };
    });
}

const escapeDots = (id: string) => id.replace(/[.]/g, "\\.");
/** An id as a regex alternative: a trailing `*` also covers the suffixed variants. */
const alternative = (id: string) => (id.endsWith("*") ? `${escapeDots(id.slice(0, -1))}(-[a-z0-9-]+)?` : escapeDots(id));

export function render(kind: string, rows: ModelRow[]): string {
  const [name, arg] = kind.split(/\s+/);
  if (name === "retired")
    return `["'\`]((anthropic|openai)[:/])?(${rows
      .filter((r) => r.status === "retired")
      .map((r) => alternative(r.id))
      .join("|")})["'\`]`;
  if (name === "rejects") {
    const ids = rows.filter((r) => r.rejects.includes(arg ?? ""));
    if (!ids.length) throw new Error(`models.tsv: no model rejects "${arg}"`);
    return `^["'](anthropic\\.)?(${ids.map((r) => alternative(r.id)).join("|")})["']$`;
  }
  if (name === "advise") {
    const dep = rows.filter((r) => r.status === "deprecated").map((r) => `${r.id}|${r.retires}|${r.replacement}`);
    const known = rows.filter((r) => r.provider === "anthropic").map((r) => r.id);
    for (const s of [...dep, ...known]) if (/['";\\]/.test(s)) throw new Error(`models.tsv: ${s} holds a quote, ";" or "\\"`);
    return `forge_advise forge_model_ids '${dep.join(";")}' '${known.join(" ")}'`;
  }
  throw new Error(`unknown models.tsv kind "${kind}"`);
}

/** A YAML single-quoted scalar. */
const yamlQuote = (s: string) => `'${s.replace(/'/g, "''")}'`;

/** The rule file with every marked line rendered from the data file; unchanged when it is current. */
export function renderRuleFile(text: string, rows: ModelRow[]): string {
  const lines = text.split("\n");
  for (let i = 0; i < lines.length; i++) {
    const m = /^\s*# models\.tsv: (.+?)\s*$/.exec(lines[i]!);
    if (!m) continue;
    const next = lines[i + 1];
    const key = next === undefined ? null : /^(\s*(?:- )?[A-Za-z_]+: )/.exec(next);
    if (!key) throw new Error(`a "# models.tsv: ${m[1]}" comment must precede a "key: value" line`);
    lines[i + 1] = key[1] + yamlQuote(render(m[1]!, rows));
  }
  return lines.join("\n");
}

/** The rule files under the packs directory that render the data file. */
export function modelRuleFiles(packsDir = join(import.meta.dir, "..", "packs")): string[] {
  const out: string[] = [];
  const walk = (dir: string) => {
    for (const name of readdirSync(dir).sort()) {
      const p = join(dir, name);
      if (statSync(p).isDirectory()) walk(p);
      else if (name.endsWith(".yaml") && readFileSync(p, "utf8").includes("# models.tsv: ")) out.push(p);
    }
  };
  walk(packsDir);
  return out;
}

if (import.meta.main) {
  const rows = loadModels();
  for (const f of modelRuleFiles()) {
    const text = readFileSync(f, "utf8");
    const next = renderRuleFile(text, rows);
    if (next !== text) {
      writeFileSync(f, next);
      console.log(`models: rendered ${f}`);
    }
  }
}
