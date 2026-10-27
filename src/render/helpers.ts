/** The helper scripts shipped to .forge/helpers/ when an active check or the types engine uses them. */
import { readdirSync } from "node:fs";
import { join } from "node:path";
import { commandsOf } from "../core/tools.js";
import { packageRoot } from "../util.js";
import type { ActiveCheck } from "./checks.js";
import type { TypesLanguage } from "./engine.js";
import { helperTemplate } from "./template.js";

/** The types engines' adapters (§15.2), shipped for the languages the active rules type-check. */
const TYPES_ADAPTERS: Record<TypesLanguage, string> = { python: "types_python.py", typescript: "types_typescript.mjs" };

/**
 * Scripts that engine helpers run with a runtime (§13.4): every file of templates/helpers/
 * except the types engines' adapters, shipped as plain files in .forge/helpers/ only when an
 * active check names it (`forge_ratchet <runtime> <name>`, `forge_helper <runtime> <name>`).
 * A project formatter may rewrite them: their integrity hash ignores formatting (ownedHash).
 */
function helperScripts(): string[] {
  const adapters = new Set(Object.values(TYPES_ADAPTERS));
  return readdirSync(join(packageRoot(), "templates", "helpers"))
    .filter((f) => !adapters.has(f))
    .sort();
}

export const HELPERS_DIR = ".forge/helpers";

/** The helper files the checks and types engines use: path (repository-relative) -> content. */
export function helperFiles(checks: ActiveCheck[], types: TypesLanguage[] = []): Map<string, string> {
  const out = new Map<string, string>();
  const commands = checks.flatMap((c) => commandsOf(c.target));
  for (const name of helperScripts()) {
    const used = new RegExp(`\\bforge_(ratchet|helper) [a-z]+ ${name.replace(".", "\\.")}(\\s|$)`);
    if (commands.some((c) => used.test(c))) out.set(`${HELPERS_DIR}/${name}`, helperTemplate(name));
  }
  for (const lang of types) out.set(`${HELPERS_DIR}/${TYPES_ADAPTERS[lang]}`, helperTemplate(TYPES_ADAPTERS[lang]));
  return out;
}
