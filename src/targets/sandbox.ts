/** `sandbox`: a setting of Claude Code's Bash sandbox, user level only (§7). */
import type { SandboxTarget } from "../types.js";
import { code } from "./events.js";
import type { TargetKind } from "./kind.js";

export const sandbox: TargetKind<SandboxTarget> = {
  kind: "sandbox",
  phase: 2,
  schema: {
    additionalProperties: false,
    required: ["path", "value"],
    properties: {
      target: true,
      path: { type: "string", pattern: "^[A-Za-z][A-Za-z0-9]*(\\.[A-Za-z][A-Za-z0-9]*)*$" },
      value: true,
    },
  },
  label: () => "sandbox",
  needsExamples: () => false,
  plan(ctx, rule, st, t) {
    if (ctx.level !== "user") {
      ctx.warnings.push(`${rule.id}: sandbox targets are user-level only`);
      return;
    }
    const path = `sandbox.${t.path}`;
    if (Array.isArray(t.value)) for (const v of t.value) ctx.out.settings.push({ file: ctx.settingsFile, path, value: v, append: true });
    else ctx.out.settings.push({ file: ctx.settingsFile, path, value: t.value, append: false });
    st.enforced.push(`sandbox ${t.path} (${ctx.settingsFile})`);
  },
  enforcement: () => ({ by: "sandbox" }),
  lifecycle: (_rule, t) => [{ step: "tool", via: "sandbox", effect: "blocks", text: `Bash runs in Claude Code's sandbox with ${code(`sandbox.${t.path}`)} = ${code(JSON.stringify(t.value))}.` }],
};
