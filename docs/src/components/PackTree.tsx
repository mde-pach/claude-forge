import { useMemo, useState } from "react";
import StatusBadge, { type Status } from "./StatusBadge";

export interface TreeNode {
  id: string;
  name: string;
  description: string;
  level: "user" | "project";
  href: string;
  rules: { id: string; status: Status }[];
  children: TreeNode[];
}

function matches(n: TreeNode, q: string): boolean {
  return n.id.includes(q) || n.description.toLowerCase().includes(q) || n.rules.some((r) => r.id.includes(q)) || n.children.some((c) => matches(c, q));
}

function worst(statuses: Status[]): Status | undefined {
  if (!statuses.length) return undefined;
  for (const s of ["failed", "not run", "skipped"] as Status[]) if (statuses.includes(s)) return s;
  return "passed";
}

function Node({ node, query, open, toggle }: { node: TreeNode; query: string; open: Set<string>; toggle: (id: string) => void }) {
  if (query && !matches(node, query)) return null;
  const expanded = query !== "" || open.has(node.id);
  const status = worst(node.rules.map((r) => r.status));
  return (
    <li>
      <div className="row">
        {node.children.length ? (
          <button type="button" aria-expanded={expanded} aria-label={`${expanded ? "Collapse" : "Expand"} ${node.id}`} onClick={() => toggle(node.id)}>
            {expanded ? "▾" : "▸"}
          </button>
        ) : (
          <span style={{ width: "1.25rem" }} />
        )}
        <a href={node.href}>
          <strong>{node.name}</strong>
        </a>
        <span className="count">
          {node.rules.length} rule{node.rules.length === 1 ? "" : "s"}
        </span>
        {status ? <StatusBadge status={status} /> : null}
        <span className="desc">{node.description}</span>
      </div>
      {node.children.length && expanded ? (
        <ul>
          {node.children.map((c) => (
            <Node key={c.id} node={c} query={query} open={open} toggle={toggle} />
          ))}
        </ul>
      ) : null}
    </li>
  );
}

/** The pack tree with search; languages first, then packages, then sub-packages. */
export default function PackTree({ roots }: { roots: TreeNode[] }) {
  const all = useMemo(() => {
    const ids: string[] = [];
    const visit = (n: TreeNode) => {
      ids.push(n.id);
      n.children.forEach(visit);
    };
    roots.forEach(visit);
    return ids;
  }, [roots]);
  const [open, setOpen] = useState(() => new Set(all));
  const [query, setQuery] = useState("");
  const toggle = (id: string) =>
    setOpen((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  return (
    <div className="forge-tree">
      <input type="search" placeholder="Filter packs and rules (e.g. react, alembic, no-query)" value={query} onChange={(e) => setQuery(e.target.value.trim().toLowerCase())} aria-label="Filter packs and rules" />
      <ul>
        {roots.map((r) => (
          <Node key={r.id} node={r} query={query} open={open} toggle={toggle} />
        ))}
      </ul>
    </div>
  );
}
