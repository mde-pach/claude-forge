// Everything the site shows comes from here: the packs as Forge's own loader reads them
// (`claude-forge packs dump --json`), its --help, and schemas/*.json. scripts/data.ts writes the
// first two into docs/.data/ before every dev or build run. Functions re-read on every call so
// the dev server follows a new dump.
import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { marked } from "marked";
import type { CliHelp, Detect, Pack, Rule } from "./types";

/** The repository root: the closest directory up from the working directory with packs/ and Cargo.toml (bundled pages cannot use import.meta.url). */
function findRoot(): string {
  for (let d = process.cwd(); ; d = dirname(d)) {
    if (existsSync(join(d, "packs")) && existsSync(join(d, "Cargo.toml"))) return d;
    if (dirname(d) === d) throw new Error("forge docs: run from inside the repository");
  }
}
export const ROOT = findRoot();
export const DATA = join(ROOT, "docs", ".data");
export const REPO_URL = "https://github.com/mde-pach/claude-forge";

function readData<T>(name: string): T {
  const file = join(DATA, name);
  if (!existsSync(file)) throw new Error(`forge docs: ${file} is missing; run \`bun run data\` in docs/ (it runs claude-forge packs dump --json)`);
  return JSON.parse(readFileSync(file, "utf8")) as T;
}

/** Every pack by id, in the loader's order. */
export function packs(): Map<string, Pack> {
  return new Map(readData<Pack[]>("packs.json").map((p) => [p.id, p]));
}

export const cliHelp = (): CliHelp[] => readData<CliHelp[]>("cli.json");

/** A schema of schemas/, parsed. */
export const schema = (name: "pack" | "rule" | "forge"): unknown => JSON.parse(readFileSync(join(ROOT, "schemas", `${name}.schema.json`), "utf8"));

/** A site URL under the configured base. */
export function url(path: string): string {
  const base = import.meta.env.BASE_URL.replace(/\/$/, "");
  return `${base}/${path.replace(/^\//, "")}`;
}

export const packUrl = (id: string) => url(`packs/${id}/`);
export const ruleAnchor = (id: string) => id.replaceAll("/", "-");

export function markdown(md: string): string {
  return marked.parse(md, { async: false });
}

/** The rule file's text, as written in packs/. */
export const ruleSource = (rule: Rule): string => readFileSync(join(ROOT, "packs", rule.file), "utf8");

export function lineage(all: Map<string, Pack>, id: string): Pack[] {
  const out: Pack[] = [];
  for (let p = all.get(id); p; p = p.parent ? all.get(p.parent) : undefined) out.unshift(p);
  return out;
}

export function leaf(id: string): string {
  return id.split("/").pop()!;
}

const values = (v: string | string[]) => (Array.isArray(v) ? v : [v]).join(" or ");

/** One chip per way the pack is detected, every kind of `detect` included. */
export function detectChips(d: Detect | undefined): string[] {
  if (!d) return [];
  return [
    ...(d.always ? ["every project"] : []),
    ...(d.files_any ?? []).map((f) => `file ${f}`),
    ...(d.sources_any ?? []).map((f) => `source ${f}`),
    ...(d.deps_any ?? []).map((x) => `${x.ecosystem} ${x.name}${x.version ? ` ${x.version}` : ""}`),
    ...(d.library_any ?? []).map((e) => `publishes a ${e} library`),
    ...(d.content_any ?? []).map((c) => `${c.files.join(", ")} containing /${c.pattern.length > 40 ? `${c.pattern.slice(0, 40)}…` : c.pattern}/`),
    ...(d.yaml_keys?.length ? [`YAML with keys ${d.yaml_keys.join(", ")}`] : []),
    ...Object.entries(d.facts_any ?? {}).map(([k, v]) => `${k}: ${values(v)}`),
  ];
}

/** A context item's condition, as chips ("deps zod >=4", "packs biome"). */
export function whenChips(w: { deps?: { ecosystem: string; name: string; version?: string }[]; files?: string[]; facts?: Record<string, string | string[]>; packs?: string[] } | undefined): string[] {
  if (!w) return [];
  return [
    ...(w.deps ?? []).map((x) => `${x.ecosystem} ${x.name}${x.version ? ` ${x.version}` : ""}`),
    ...(w.files ?? []).map((f) => `file ${f}`),
    ...Object.entries(w.facts ?? {}).map(([k, v]) => `${k}: ${values(v)}`),
    ...(w.packs?.length ? [`with ${w.packs.join(" or ")}`] : []),
  ];
}

export type { Pack, Rule };
