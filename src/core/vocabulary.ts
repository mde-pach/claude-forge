/**
 * The words packs, rules and forge.yaml use, one list each: the TypeScript types
 * (src/types.ts) and the JSON Schemas (src/schemas.ts, src/targets/) derive from them, so the
 * two cannot disagree on what a level, a stage or a mode is.
 */

export const LEVELS = ["user", "project"] as const;
export const LANGUAGES = ["python", "typescript", "tsx", "yaml", "any"] as const;
export const DECISIONS = ["deny", "ask"] as const;
export const ECOSYSTEMS = ["pypi", "npm"] as const;
/** check.sh's stages, in the order it runs them (§10.1). */
export const STAGES = ["format", "lint", "codegen", "typecheck", "migration", "test"] as const;
/** How an ast-grep or grep match turns into a failure (§6.2). */
export const MODES = ["forbid", "no-increase", "no-decrease", "advise"] as const;
/** Built-in guard checks: guard_check_<name> in guard.sh decides on the parsed command (§9.2). */
export const GUARD_CHECKS = ["ephemeral-exec"] as const;
/** Hook managers Forge adds its pre-commit block to (§10.3). */
export const HOOK_MANAGERS = ["pre-commit", "husky", "lefthook", "hooks"] as const;
