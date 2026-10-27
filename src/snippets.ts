// The config snippets forge status prints for unsatisfied audits (§11), merged per tool:
// several rules add entries to the same TOML table (ruff's extend-select, its
// flake8-tidy-imports banned-api table, ty's rules), and a table written twice is not
// valid TOML, so the snippets of one tool are shown as one block too.
import { parse as parseToml } from "smol-toml";

type Toml = Record<string, unknown>;

/** A snippet as rules write it: TOML whose lines may be "[table] key = value" (a header, then its pair). */
function parseSnippet(snippet: string): Toml {
  return parseToml(snippet.replace(/^(\s*\[[^\]]+\])[ \t]+(?=[^\s#])/gm, "$1\n")) as Toml;
}

const isTable = (v: unknown): v is Toml => typeof v === "object" && v !== null && !Array.isArray(v) && !(v instanceof Date);

/** b merged into a: tables merged key by key, arrays joined without repeats, other values from b. */
function merge(a: Toml, b: Toml): Toml {
  const out: Toml = { ...a };
  for (const [k, v] of Object.entries(b)) {
    const old = out[k];
    if (isTable(old) && isTable(v)) out[k] = merge(old, v);
    else if (Array.isArray(old) && Array.isArray(v)) out[k] = [...old, ...v.filter((x) => !old.some((o) => JSON.stringify(o) === JSON.stringify(x)))];
    else out[k] = v;
  }
  return out;
}

const key = (k: string) => (/^[A-Za-z0-9_-]+$/.test(k) ? k : JSON.stringify(k));
const value = (v: unknown): string => (Array.isArray(v) ? `[${v.map(value).join(", ")}]` : typeof v === "string" ? JSON.stringify(v) : String(v));

/** TOML for a merged table: a header only for the tables that hold values. */
function serialize(t: Toml, path: string[] = []): string[] {
  const lines: string[] = [];
  const tables: [string, Toml][] = [];
  for (const [k, v] of Object.entries(t)) {
    if (isTable(v)) tables.push([k, v]);
    else lines.push(`${key(k)} = ${value(v)}`);
  }
  const out = lines.length && path.length ? [`[${path.map(key).join(".")}]`, ...lines] : lines;
  for (const [k, v] of tables) {
    const sub = serialize(v, [...path, k]);
    if (sub.length) out.push(...(out.length ? [""] : []), ...sub);
  }
  return out;
}

/** The snippets of one TOML tool as one block, or undefined when one does not parse. */
export function mergeTomlSnippets(snippets: string[]): string | undefined {
  try {
    return serialize(snippets.map(parseSnippet).reduce(merge, {})).join("\n");
  } catch {
    return undefined;
  }
}
