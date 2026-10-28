import { detectAnywhere, lineage } from "../packs.js";
import type { PackStatus, PlanOptions } from "./types.js";

/**
 * The active packs (§5.1) and why each is active: added in forge.yaml (with its
 * ancestors), detected under an active parent, or required by an active pack; then
 * the ones forge.yaml removes, with their subtree and every pack requiring them.
 */
export function activePacks(opts: PlanOptions, warnings: string[]): { packs: PackStatus[]; removed: PackStatus[] } {
  const all = [...opts.packs.values()];
  if (opts.level === "user") return { packs: all.filter((p) => p.level === "user").map((p) => ({ id: p.id, why: "user level" })), removed: [] };
  const why = new Map<string, string>();
  const activate = (id: string, reason: string) => {
    if (why.has(id)) return false;
    why.set(id, reason);
    return true;
  };
  // forge.yaml add: the pack and its ancestors
  for (const id of opts.config?.packs?.add ?? []) {
    const p = opts.packs.get(id);
    if (!p) warnings.push(`forge.yaml: packs.add: unknown pack "${id}"`);
    else if (p.level !== "project") warnings.push(`forge.yaml: packs.add: "${id}" is a user-level pack (use forge apply --user)`);
    else {
      for (const a of lineage(opts.packs, id).slice(0, -1)) activate(a, `parent of ${id}`);
      activate(id, "forge.yaml");
    }
  }
  for (const id of opts.config?.packs?.remove ?? []) if (!opts.packs.has(id)) warnings.push(`forge.yaml: packs.remove: unknown pack "${id}"`);
  // detection down the tree (a child only under an active parent), and requires, to a fixpoint
  for (let changed = true; changed; ) {
    changed = false;
    for (const p of all) {
      if (p.level !== "project" || why.has(p.id)) continue;
      if ((p.parent === undefined || why.has(p.parent)) && opts.facts && detectAnywhere(p, opts.facts)) changed = activate(p.id, "detected") || changed;
    }
    for (const id of [...why.keys()]) for (const req of opts.packs.get(id)!.requires) for (const a of lineage(opts.packs, req)) changed = activate(a, `required by ${id}`) || changed;
  }
  // removal: the pack, its subtree, and every pack requiring a removed one
  const removed = new Map<string, string>();
  for (const id of opts.config?.packs?.remove ?? []) if (why.has(id)) removed.set(id, "removed in forge.yaml");
  for (let changed = true; changed; ) {
    changed = false;
    for (const id of why.keys()) {
      if (removed.has(id)) continue;
      const p = opts.packs.get(id)!;
      const req = p.requires.find((r) => removed.has(r));
      const reason = p.parent && removed.has(p.parent) ? `parent ${p.parent} is not active` : req ? `requires removed pack ${req}` : undefined;
      if (reason) {
        removed.set(id, reason);
        changed = true;
      }
    }
  }
  const order = (a: string, b: string) => (a < b ? -1 : a > b ? 1 : 0);
  const packs = [...why.entries()]
    .filter(([id]) => !removed.has(id))
    .sort(([a], [b]) => order(a, b))
    .map(([id, w]) => ({ id, why: w }));
  return { packs, removed: [...removed.entries()].sort(([a], [b]) => order(a, b)).map(([id, w]) => ({ id, why: w })) };
}
