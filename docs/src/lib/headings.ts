import type { SpecNode } from "./spec";

export const slugify = (s: string) =>
  s
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-|-$/g, "");

/** Table-of-contents entries for the describe blocks (as SpecNodes renders them). */
export function specHeadings(nodes: SpecNode[], depth = 2): { depth: number; slug: string; text: string }[] {
  return nodes.flatMap((n) =>
    n.kind === "describe" && depth <= 3
      ? [{ depth, slug: `${slugify(n.title)}-${n.line}`, text: (n.title.charAt(0).toUpperCase() + n.title.slice(1)).replace(/`/g, "") }, ...specHeadings(n.children, depth + 1)]
      : [],
  );
}
