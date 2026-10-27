import { loadLocalRules, loadProjectConfig, readProjectFacts } from "../packs.js";
import { resolveRoles } from "../roles.js";
import { listRepoFiles } from "../util.js";
import type { PlanOptions } from "./types.js";

/** Reads the project inputs (forge.yaml, .forge/rules, dependency facts, file roles) a project plan needs. */
export function readProjectInputs(repo: string): Pick<PlanOptions, "config" | "localRules" | "facts" | "roles"> {
  const config = loadProjectConfig(repo);
  const files = listRepoFiles(repo);
  return {
    config,
    localRules: loadLocalRules(repo),
    facts: readProjectFacts(repo, () => files),
    roles: resolveRoles({ repo, files }, config?.files),
  };
}
