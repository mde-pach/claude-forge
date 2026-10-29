// The history of the ruff rules Forge's packs ask for (src/adapters/ruff-rules.ts), read from
// ruff itself: `ruff rule --all --output-format json` of each release of RUFF_MATRIX. For
// each code a `tool: ruff` target expects:
//   since   the first release of the matrix where the rule is stable (not preview), under
//           this code or an earlier one, and reports the match examples of the targets
//           expecting it (src/adapters/ruff-examples.ts);
//   codes   the codes the same rule had (ruff renames codes: TCH004 -> TC004 in 0.8.0,
//           TRIO1xx -> ASYNC1xx and ASYNC100 -> ASYNC210 in 0.5.0, TRY302 -> TRY203);
//   names   the names it had (ruff renames rules too: PYI019).
// A rule is the same across releases when it has the same name, or the same code while its
// current name does not exist yet and its old name is no current rule's (ASYNC100 was
// blocking-http-call-in-async-function, now ASYNC210's: not the same rule).
//
//   bun scripts/ruff-rules.ts            rewrite src/adapters/ruff-rules.ts
//   bun scripts/ruff-rules.ts --check    fail when it differs (tests/spec/16-adapters.test.ts)
import { execFileSync, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { loadPacks } from "../src/packs";
import { RUFF_MATRIX } from "../src/adapters/ruff";
import { reportsEveryFile, ruffExampleProject } from "../src/adapters/ruff-examples";
import type { Rule, ToolTarget } from "../src/types";

interface RuffRule {
  code: string;
  name: string;
  preview?: boolean;
}

export const OUT = join(import.meta.dir, "..", "src", "adapters", "ruff-rules.ts");
const CACHE = join(process.env.XDG_CACHE_HOME ?? join(homedir(), ".cache"), "forge", "ruff-rules");

/** ruff <version>'s rule table, from its own `ruff rule --all` (cached). */
export function ruffTable(version: string): RuffRule[] {
  const f = join(CACHE, `${version}.json`);
  if (!existsSync(f)) {
    mkdirSync(CACHE, { recursive: true });
    const out = execFileSync("uvx", ["-q", `ruff==${version}`, "rule", "--all", "--output-format", "json"], { maxBuffer: 1 << 28 }).toString();
    writeFileSync(f, out);
  }
  return (JSON.parse(readFileSync(f, "utf8")) as RuffRule[]).map(({ code, name, preview }) => ({ code, name, preview: !!preview }));
}

/** The `tool: ruff` targets of Forge's packs, with their rules. */
export function ruffTargets(): { rule: Rule; t: ToolTarget }[] {
  return [...loadPacks(join(import.meta.dir, "..", "packs")).values()].flatMap((p) =>
    p.rules.flatMap((rule) => rule.enforce.filter((t): t is ToolTarget => t.target === "tool" && t.tool === "ruff").map((t) => ({ rule, t }))),
  );
}

/** The codes `tool: ruff` targets of Forge's packs expect. */
export function expectedCodes(): string[] {
  return [...new Set(ruffTargets().map((x) => x.t.expects))].sort();
}

/** Whether ruff <version>, selecting <installed> (its code for the rule), reports the match examples of every target expecting <code> (cached). */
function reportsExamples(version: string, code: string, installed: string): boolean {
  const f = join(CACHE, "examples", `${version}-${code}-${installed}.json`);
  if (existsSync(f)) return JSON.parse(readFileSync(f, "utf8")) as boolean;
  let ok = true;
  for (const { rule, t } of ruffTargets().filter((x) => x.t.expects === code)) {
    const { dir, files } = ruffExampleProject(rule, t, installed);
    if (!files.length) continue;
    const r = spawnSync("uvx", ["-q", `ruff==${version}`, "check", "--no-cache", "--output-format", "json", ...files], { cwd: dir, encoding: "utf8" });
    if (!reportsEveryFile(r.stdout, installed, files)) ok = false;
  }
  mkdirSync(join(CACHE, "examples"), { recursive: true });
  writeFileSync(f, JSON.stringify(ok));
  return ok;
}

const cmp = (a: string, b: string) => {
  const x = a.split(".").map(Number);
  const y = b.split(".").map(Number);
  for (let i = 0; i < 3; i++) if ((x[i] ?? 0) !== (y[i] ?? 0)) return (x[i] ?? 0) - (y[i] ?? 0);
  return 0;
};

export function ruffHistory(): Record<string, { since: string; codes: string[]; names: string[] }> {
  const versions = [...RUFF_MATRIX].sort(cmp);
  const tables = new Map(versions.map((v) => [v, ruffTable(v)]));
  const reference = tables.get(versions.at(-1)!)!;
  const currentNames = new Set(reference.map((r) => r.name));
  const out: Record<string, { since: string; codes: string[]; names: string[] }> = {};
  for (const code of expectedCodes()) {
    const now = reference.find((r) => r.code === code);
    if (!now) throw new Error(`ruff ${versions.at(-1)} has no rule ${code}`);
    const codes = new Set([code]);
    const names = new Set([now.name]);
    // the same rule: one of its names, or one of its codes under a name no current rule has; to a fixed point
    const matches = new Map<string, RuffRule>();
    for (let grown = true; grown; ) {
      grown = false;
      for (const v of versions) {
        if (matches.has(v)) continue;
        const t = tables.get(v)!;
        const same = t.find((r) => names.has(r.name)) ?? t.find((r) => codes.has(r.code) && !t.some((x) => names.has(x.name)) && !currentNames.has(r.name));
        if (!same) continue;
        matches.set(v, same);
        if (!codes.has(same.code) || !names.has(same.name)) grown = true;
        codes.add(same.code);
        names.add(same.name);
      }
    }
    // the first release where the rule is stable and reports the match examples of every target expecting it
    // (a rule may exist before it covers what Forge asks of it: flake8-trio's TRIO110 did not flag asyncio)
    const since = versions.find((v) => matches.has(v) && !matches.get(v)!.preview && reportsExamples(v, code, matches.get(v)!.code)) ?? "";
    if (!since) throw new Error(`ruff ${code} is stable in no release of RUFF_MATRIX`);
    out[code] = { since, codes: [...codes].sort((a, b) => (a === code ? -1 : b === code ? 1 : a < b ? -1 : 1)), names: [...names] };
  }
  return out;
}

/** Biome's formatting of a TypeScript text (the repository's formatter). */
function formatted(text: string): string {
  return execFileSync(join(import.meta.dir, "..", "node_modules", ".bin", "biome"), ["format", `--stdin-file-path=${OUT}`], { input: text }).toString();
}

export function render(): string {
  const h = ruffHistory();
  const lines = Object.entries(h).map(([c, v]) => `  ${c}: { since: "${v.since}", codes: [${v.codes.map((x) => `"${x}"`).join(", ")}], names: [${v.names.map((x) => `"${x}"`).join(", ")}] },`);
  return [
    "// Generated by scripts/ruff-rules.ts from `ruff rule --all --output-format json` of each release in RUFF_MATRIX",
    "// (src/adapters/ruff.ts). Do not edit: run `bun scripts/ruff-rules.ts`.",
    "",
    "/** Per code a `tool: ruff` target expects: the first stable release, and the codes and names the same rule had. */",
    "export const RUFF_RULES: Record<string, { since: string; codes: string[]; names: string[] }> = {",
    ...lines,
    "};",
    "",
  ].join("\n");
}

export const rendered = () => formatted(render());

if (import.meta.main) {
  const text = rendered();
  if (process.argv.includes("--check")) {
    if (!existsSync(OUT) || readFileSync(OUT, "utf8") !== text) {
      console.error(`${OUT} differs from ruff's own history: run bun scripts/ruff-rules.ts`);
      process.exit(1);
    }
  } else writeFileSync(OUT, text);
}
