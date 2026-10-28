import {
  ENGINE_PYTHON,
  packageRoot,
  HADOLINT_SHA256,
  HADOLINT_VERSION,
  LOCKFILE_LINT_VERSION,
  REDOCLY_VERSION,
  releaseTable,
  sha256,
  SPECTRAL_VERSION,
  SQUAWK_VERSION,
  TFLINT_RULESETS,
  TY_VERSION,
  ZIZMOR_VERSION,
} from "../util.js";
import type { AstGrepTarget, Rule } from "../types.js";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fill, helperTemplate } from "./template.js";

/** The languages of the types engine (§15.2). */
export type TypesLanguage = "python" | "typescript";

/** The types engine a rule's ast-grep target needs (§15.2): its language's, when it has a where. */
export function typesLanguage(rule: Rule, target: AstGrepTarget): TypesLanguage | undefined {
  if (!target.where) return undefined;
  return rule.scope.language === "python" ? "python" : "typescript";
}

/** A hash of the engine adapters: a daemon started by another revision is restarted. */
export function typesRevision(): string {
  const text = ["types_python.py", "types_typescript.mjs"].map(helperTemplate).join("\0");
  return sha256(text).slice(0, 12);
}

// ---------------------------------------------------------------- modules (templates/engine)

/**
 * A module of the check engine (templates/engine/<name>.sh): shell functions and the
 * constants they read. What it provides and what it uses are read from its text, so a new
 * function is found where it is written; nothing lists them by hand.
 */
export interface EngineModule {
  name: string;
  text: string;
  /** Its functions (forge_*) and constants (FORGE_*). */
  provides: Set<string>;
  /** The forge_* and FORGE_* names it refers to that it does not provide. */
  uses: Set<string>;
}

/** The modules every engine starts with; the others follow in name order. */
const FIRST = ["core", "tools", "advice", "tests", "types"];

const NAMES = /\b(?:forge_[a-z0-9_]+|FORGE_[A-Z0-9_]+)\b/g;

/** The names a shell text refers to (forge_* functions, FORGE_* variables). */
export function engineNames(text: string): Set<string> {
  return new Set(text.match(NAMES) ?? []);
}

let modules: EngineModule[] | undefined;
export function engineModules(): EngineModule[] {
  if (modules) return modules;
  const dir = join(packageRoot(), "templates", "engine");
  const names = readdirSync(dir)
    .filter((f) => f.endsWith(".sh"))
    .map((f) => f.slice(0, -3));
  const order = [...FIRST.filter((n) => names.includes(n)), ...names.filter((n) => !FIRST.includes(n)).sort()];
  modules = order.map((name) => {
    const text = readFileSync(join(dir, `${name}.sh`), "utf8").trimEnd();
    const provides = new Set([...text.matchAll(/^(forge_[a-z0-9_]+)\(\)/gm), ...text.matchAll(/^(FORGE_[A-Z0-9_]+)=/gm)].map((m) => m[1]!));
    const uses = new Set([...engineNames(text)].filter((n) => !provides.has(n)));
    return { name, text, provides, uses };
  });
  const seen = new Map<string, string>();
  for (const m of modules)
    for (const n of m.provides) {
      if (seen.has(n)) throw new Error(`templates/engine: ${n} is defined in both ${seen.get(n)}.sh and ${m.name}.sh`);
      seen.set(n, m.name);
    }
  return modules;
}

/**
 * The modules the given scripts need: core, the modules providing a name they use, and
 * theirs in turn, in engine order. Names no module provides (lib.sh's, the generated
 * checks' forge_check_<n>, forge_scope_<n>) are left to whoever defines them.
 */
export function resolveEngine(texts: string[]): string[] {
  const all = engineModules();
  const provider = new Map(all.flatMap((m) => [...m.provides].map((n) => [n, m] as const)));
  const want = new Set<string>(["core"]);
  const queue = [...texts.flatMap((t) => [...engineNames(t)]), ...all.find((m) => m.name === "core")!.uses];
  while (queue.length) {
    const m = provider.get(queue.pop()!);
    if (!m || want.has(m.name)) continue;
    want.add(m.name);
    queue.push(...m.uses);
  }
  return all.filter((m) => want.has(m.name)).map((m) => m.name);
}

/** One module with Forge's pins filled in (the types engines' pins depend on the languages used). */
export function renderEngineModule(name: string, types: TypesLanguage[] = []): string {
  const m = engineModules().find((x) => x.name === name);
  if (!m) throw new Error(`templates/engine: no module ${name}`);
  return fill(m.text, pins(types));
}

/**
 * The engine: the given modules (all of them by default), in engine order, with Forge's pins
 * filled in. It depends on the plan through the modules its checks use and the types
 * engines it starts.
 */
export function renderEngine(types: TypesLanguage[] = [], names: string[] = engineModules().map((m) => m.name)): string {
  return engineModules()
    .filter((m) => names.includes(m.name))
    .map((m) => renderEngineModule(m.name, types))
    .join("\n\n");
}

/** The values of the engine's @@KEY@@ placeholders. */
function pins(types: TypesLanguage[]): Record<string, string> {
  return {
    SQUAWK_VERSION,
    REDOCLY_VERSION,
    SPECTRAL_VERSION,
    LOCKFILE_LINT_VERSION,
    ZIZMOR_VERSION,
    HADOLINT_VERSION,
    HADOLINT_SHAS: Object.entries(HADOLINT_SHA256)
      .map(([p, h]) => `${p} ${h}`)
      .join("\n"),
    RELEASES: releaseTable(),
    TFLINT_RULESETS: Object.entries(TFLINT_RULESETS)
      .map(([p, v]) => `${p} ${v}`)
      .join("\n"),
    // newline-separated: IFS is a newline in the generated scripts
    TYPES_LANGS: types.join("\n"),
    TY_VERSION,
    ENGINE_PYTHON,
    TYPES_REV: types.length ? typesRevision() : "",
  };
}
