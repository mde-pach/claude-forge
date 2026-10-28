import { computeChanges, printDiff } from "../changes.js";
import { context } from "../context.js";
import type { Flags } from "../flags.js";
import { out } from "../io.js";

export function cmdDiff(flags: Flags): number {
  const ctx = context(flags);
  const ch = computeChanges(ctx);
  for (const path of ch.drift) out(`forge: ${path} was modified since the last apply`);
  for (const path of ch.conflicts) out(`forge: ${path} exists and is not a forge file`);
  return printDiff(ctx, ch, { withLock: true }) || ch.drift.length > 0 ? 1 : 0;
}
