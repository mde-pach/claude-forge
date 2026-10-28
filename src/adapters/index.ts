/**
 * Tool adapters (§16): how Forge drives each version of a tool. A rule says what it needs
 * from a tool (an operation: "lint", "format-check", "typecheck"; a code a `tool` target
 * expects); the adapter of the installed version says how to ask for it, or that this version
 * cannot. The generated engine carries the same table (templates/engine/adapters.sh).
 */
import { compareVersions } from "../core/versions.js";
import { RUFF } from "./ruff.js";
import { TY } from "./ty.js";
import type { AdapterRange, ToolAdapter } from "./types.js";

export type { AdapterRange, ToolAdapter } from "./types.js";

export const ADAPTERS: ToolAdapter[] = [RUFF, TY];

export function adapterOf(tool: string): ToolAdapter | undefined {
  return ADAPTERS.find((a) => a.tool === tool);
}

/** The range covering <version>, or undefined (older than the first, or a gap). */
export function rangeOf(a: ToolAdapter, version: string): AdapterRange | undefined {
  return a.ranges.find((r) => compareVersions(version, r.from) >= 0 && (!r.to || compareVersions(version, r.to) < 0));
}

/** The first version from which <op> is possible, at or after <version>; undefined when none. */
export function opSince(a: ToolAdapter, op: string, version = "0"): string | undefined {
  return a.ranges.find((r) => (!r.to || compareVersions(version, r.to) < 0) && typeof r.ops[op] === "string")?.from;
}

/**
 * The adapters as the engine's table, one line per range and operation:
 * tool, from, to ("" when open), operation, arguments ("-" when the range cannot).
 */
export function adapterTable(): string {
  return ADAPTERS.flatMap((a) => {
    // every range lists every operation of the tool: one it does not have cannot
    const ops = [...new Set(a.ranges.flatMap((r) => Object.keys(r.ops)))].sort();
    return a.ranges.flatMap((r) => ops.map((op) => [a.tool, r.from, r.to ?? "", op, r.ops[op] ?? "-"].join("\t")));
  }).join("\n");
}

/** The operations check commands ask of adapted tools: tool, operation, rule id; one line each. */
export function adapterUses(commands: { rule: string; command: string }[]): string {
  const out = new Set<string>();
  for (const { rule, command } of commands) for (const m of command.matchAll(/\bforge_do\s+([a-z][a-z0-9-]*)\s+([a-z][a-z0-9-]*)/g)) out.add([m[1], m[2], rule].join("\t"));
  return [...out].join("\n");
}

/** tool, pinned version (Forge's own tools), the version proven last, the usage-error exit status and pattern; one line each. */
export function adapterMeta(): string {
  return ADAPTERS.map((a) => [a.tool, a.pinned ?? "", a.verified, String(a.usage.exit), a.usage.pattern].join("\t")).join("\n");
}

/**
 * The codes <codes> of <tool> as the engine's table: tool, code, first stable release,
 * the codes the same rule had (comma-separated), its names (comma-separated).
 */
export function codeTable(tool: string, codes: string[]): string {
  const h = adapterOf(tool)?.codes?.history ?? {};
  return [...new Set(codes)]
    .sort()
    .filter((c) => h[c])
    .map((c) => [tool, c, h[c]!.since, h[c]!.codes.join(","), h[c]!.names.join(",")].join("\t"))
    .join("\n");
}

/**
 * How a check command names an operation: `forge_do <tool> <op>`. Commands that name an
 * adapted tool directly (`ruff check …`) are rejected when the rule is loaded.
 */
export const DO = "forge_do";
