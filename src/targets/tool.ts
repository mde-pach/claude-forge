/** `tool`: a diagnostic of the project's own linter or type checker, audited in its config (§11). */
import { AUDITED_TOOLS } from "../core/tools.js";
import type { ToolTarget } from "../types.js";
import { code, type LifecycleEvent } from "./events.js";
import type { TargetKind } from "./kind.js";

/** Where a tool target's diagnostics are enforced once the audit passes. */
const TOOL_STAGE: Record<ToolTarget["tool"], { stage: "lint" | "typecheck"; check: string }> = {
  ruff: { stage: "lint", check: "python/lint" },
  biome: { stage: "lint", check: "typescript/format-lint" },
  ty: { stage: "typecheck", check: "python/typecheck" },
  tsc: { stage: "typecheck", check: "typescript/typecheck" },
  eslint: { stage: "lint", check: "typescript/eslint/lint" },
};

export const tool: TargetKind<ToolTarget> = {
  kind: "tool",
  phase: 0,
  schema: {
    additionalProperties: false,
    required: ["tool", "expects", "snippet"],
    properties: {
      target: true,
      tool: { enum: [...AUDITED_TOOLS] },
      expects: { type: "string", minLength: 1 },
      snippet: { type: "string", minLength: 1 },
    },
  },
  label: (t) => `${t.tool} audit`,
  needsExamples: () => true,
  plan(ctx, rule, st, t) {
    const off = ctx.switchedOff(t);
    if (off) {
      off.rules.push(rule.id);
      st.notEnforced.push(`not enforced here: ${t.tool} ${t.expects} switched off by ${off.pack} (${off.reason})`);
    } else if (ctx.present(t.tool)) {
      st.audits.push(t);
      st.enforced.push(`${t.tool} (${t.expects}), audited`);
    } else st.notEnforced.push(`not enforced here: ${t.tool} not configured`);
  },
  // the audits' results say whether the tool enforces it (forge status reads them first)
  enforcement: () => ({}),
  lifecycle(_rule, t) {
    const s = TOOL_STAGE[t.tool];
    const out: LifecycleEvent[] = [
      {
        step: "apply",
        effect: "audits",
        text: `Checks that your own ${t.tool} config reports ${code(t.expects)} as an error that fails ${t.tool} on the examples below (a warning is not enough); if not, prints the config snippet to add.`,
      },
    ];
    if (s.stage === "lint") out.push({ step: "edit", effect: "feedback", text: `Your ${t.tool}, run on the edited file by ${code(s.check)}, reports ${code(t.expects)} to Claude.` });
    else out.push({ step: "edit", effect: "wakes", text: `Your ${t.tool}, run in the background by ${code(s.check)}, reports ${code(t.expects)} and wakes Claude.` });
    for (const step of ["stop", "commit", "ci"] as const) out.push({ step, effect: "blocks", text: `${code(s.check)} fails on ${code(t.expects)}.` });
    return out.map((e) => ({ ...e, condition: `with ${t.tool}` }));
  },
};
