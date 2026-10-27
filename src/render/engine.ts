import {
  ENGINE_PYTHON,
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
import { fill, helperTemplate, template } from "./template.js";

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

/** engine.sh with Forge's pins filled in; it depends on the plan only through the types engines it starts. */
export function renderEngine(types: TypesLanguage[] = []): string {
  return fill(template("engine.sh"), {
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
  }).trimEnd();
}
