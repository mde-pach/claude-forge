import { casePatterns } from "../core/globs.js";
import type { Roles } from "../roles.js";

/** The file roles engine helpers name themselves (forge_dockerfiles: @dockerfile, forge_alembic_files: @alembic-migrations): always rendered. */
export const ENGINE_ROLES = ["dockerfile", "alembic-migrations"];

/**
 * forge_role <@role> <path> (§15.4): succeeds when the repository path is one of the role's
 * files, by the effective patterns rendered literally (exclusions first).
 */
export function renderRoles(roles: Roles): string[] {
  const out = ["# File roles (§15.4): forge_role <@role> <path>.", "forge_role() {", "  case ${1#@} in"];
  for (const [name, role] of [...roles.entries()].sort(([a], [b]) => (a < b ? -1 : 1))) {
    const inc = casePatterns(role.patterns.filter((p) => !p.exclude).map((p) => p.glob));
    const exc = casePatterns(role.patterns.filter((p) => p.exclude).map((p) => p.glob));
    const body = [exc ? `case $2 in ${exc}) return 1 ;; esac` : "", inc ? `case $2 in ${inc}) return 0 ;; esac` : ""].filter(Boolean);
    out.push(`    ${name}) ${body.length ? body.join("; ") + " " : ""};;`);
  }
  out.push("  esac", "  return 1", "}");
  return out;
}
