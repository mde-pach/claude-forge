/** Which rules apply here (§5.3, §6.1): the active packs' rules, local rules, forge.yaml's disables, version gates and file roles. */
import { applyRoles, defaultRoles, type Roles, rolesOfRule } from "../roles.js";
import type { Rule } from "../types.js";
import { gatedOut, type Site } from "./placement.js";
import type { Plan, PlanOptions } from "./types.js";

export interface SelectedRules {
  /** The rules to enforce, sorted by id, with their file roles replaced by their effective patterns. */
  rules: Rule[];
  disabled: Plan["disabled"];
  deprecated: Plan["deprecated"];
  gated: Plan["gated"];
  /** Each file role the rules name, with the rules naming it. */
  roleUse: Map<string, string[]>;
  /** Rules left without files: a role they select files by has no pattern. */
  emptyRoles: Map<string, string[]>;
  roles: Roles;
}

export function selectRules(opts: PlanOptions, s: Site, warnings: string[]): SelectedRules {
  const project = opts.level === "project";
  // pack rules of active packs, replaced by local rules with the same id
  const byId = new Map<string, Rule>();
  for (const p of s.active) for (const r of opts.packs.get(p.id)!.rules) byId.set(r.id, r);
  if (project)
    for (const r of opts.localRules ?? []) {
      if (byId.has(r.id) || !opts.packs.has(r.pack) || s.active.some((p) => p.id === r.pack)) byId.set(r.id, r);
      else warnings.push(`${r.file}: pack "${r.pack}" is not active here; rule ${r.id} is ignored`);
    }

  const disabled: Plan["disabled"] = [];
  for (const d of project ? (opts.config?.disable ?? []) : []) {
    if (!byId.has(d.id)) warnings.push(`forge.yaml: disable: rule "${d.id}" is not active here`);
    else {
      byId.delete(d.id);
      disabled.push(d);
    }
  }
  const deprecated: Plan["deprecated"] = [];
  for (const [id, r] of byId)
    if (r.deprecated) {
      byId.delete(id);
      deprecated.push({ id, replaced_by: r.deprecated.replaced_by });
    }
  const gated: Plan["gated"] = [];
  for (const [id, r] of byId)
    if (project && gatedOut(s, r)) {
      byId.delete(id);
      gated.push({ id, needs: r.when!.deps.map((d) => `${d.name} ${d.version}`).join(", ") });
    }

  // file roles (§15.4): each rule as apply renders it, its roles replaced by their effective patterns
  const roles = opts.roles ?? defaultRoles();
  const roleUse = new Map<string, string[]>();
  const emptyRoles = new Map<string, string[]>();
  const rules = [...byId.values()]
    .sort((a, b) => a.id.localeCompare(b.id))
    .map((r) => {
      for (const name of rolesOfRule(r)) roleUse.set(name, [...(roleUse.get(name) ?? []), r.id]);
      const { rule, empty } = applyRoles(r, roles);
      if (empty.length) emptyRoles.set(r.id, empty);
      return rule;
    });
  return { rules, disabled, deprecated, gated, roleUse, emptyRoles, roles };
}
