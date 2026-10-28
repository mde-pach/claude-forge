/** forge explain <rule-id>: the rule file and where it is enforced here. */
import { readFileSync } from "node:fs";
import { auditRules } from "../../audit.js";
import { loadLocalRules } from "../../packs.js";
import { gitToplevel } from "../../util.js";
import { context, shippedPacks } from "../context.js";
import type { Flags } from "../flags.js";
import { ForgeError, out } from "../io.js";
import { formatAudit } from "../report.js";

export function cmdExplain(id: string | undefined, flags: Flags): number {
  if (!id) throw new ForgeError("usage: forge explain <rule-id>", 2);
  const packs = shippedPacks();
  const top = gitToplevel(process.cwd());
  let rule = [...packs.values()].flatMap((p) => p.rules).find((r) => r.id === id);
  if (top) rule = loadLocalRules(top).find((r) => r.id === id) ?? rule;
  if (!rule) throw new ForgeError(`forge: unknown rule ${id}`);
  out(`# ${rule.file}`);
  process.stdout.write(readFileSync(rule.file, "utf8"));
  out("");
  if (!top && !flags.user) {
    out("Not in a git repository: no project-specific enforcement to show.");
    return 0;
  }
  const ctx = context(flags);
  const st = ctx.plan.active.find((s) => s.rule.id === id);
  const dis = ctx.plan.disabled.find((d) => d.id === id);
  const gate = ctx.plan.gated.find((g) => g.id === id);
  if (dis) out(`Disabled here: ${dis.reason}`);
  else if (gate) out(`Not active here: the rule needs ${gate.needs}.`);
  else if (!st) out(`Not active here: pack ${rule.pack} is not active at ${ctx.level} level.`);
  else {
    out("Enforced here:");
    for (const e of st.enforced) out(`  ${e}`);
    for (const n of st.notEnforced) out(`  ${n}`);
    if (!st.enforced.length) out("  nowhere");
    if (ctx.level === "project") for (const a of auditRules(ctx.base, [st], (t) => ctx.plan.toolDirs[t], ctx.plan.engineSettings)) out(formatAudit(a));
  }
  return 0;
}
