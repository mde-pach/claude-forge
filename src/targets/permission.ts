/** `permission`: a Claude Code permissions entry, deny or ask, appended to the settings (§8.3). */
import { codeExamples } from "../examples.js";
import { permissionMatches } from "../selftest/runners.js";
import type { PermissionTarget } from "../types.js";
import { code, type LifecycleEvent } from "./events.js";
import type { TargetKind } from "./kind.js";

export const permission: TargetKind<PermissionTarget> = {
  kind: "permission",
  phase: 2,
  schema: {
    additionalProperties: false,
    required: ["decision", "rule"],
    properties: {
      target: true,
      decision: { enum: ["deny", "ask"] },
      rule: { type: "string", pattern: "^[A-Za-z]+(\\(.+\\))?$" },
    },
  },
  label: (t) => `permission ${t.decision}`,
  needsExamples: () => true,
  plan(ctx, _rule, st, t) {
    ctx.out.settings.push({ file: ctx.settingsFile, path: `permissions.${t.decision}`, value: t.rule, append: true });
    st.enforced.push(`permission ${t.decision} ${t.rule} (${ctx.settingsFile})`);
  },
  enforcement: (_st, t) => ({ by: `permission ${t.decision}` }),
  // one event per decision, listing every permission string: see lifecycleLead
  lifecycle: () => [],
  lifecycleLead(_rule, targets) {
    const events: LifecycleEvent[] = [];
    for (const decision of ["deny", "ask"] as const) {
      const specs = targets.filter((t) => t.decision === decision).map((t) => t.rule);
      if (specs.length)
        events.push({
          step: "tool",
          via: "permission",
          effect: decision === "deny" ? "blocks" : "asks",
          text: `Claude Code ${decision === "deny" ? "refuses" : "asks you before"} ${specs.map(code).join(", ")} (permission ${decision}).`,
        });
    }
    return events;
  },
  selftest(rules, { report }) {
    const { failures, counts } = report;
    counts.permission = rules.length;
    for (const r of rules) {
      const specs = r.enforce.filter((t): t is PermissionTarget => t.target === "permission").map((t) => t.rule);
      for (const ex of codeExamples(r, "match")) if (!specs.some((s) => permissionMatches(s, ex))) failures.push(`${r.id}: no permission rule matches ${ex}`);
      for (const ex of codeExamples(r, "no_match")) {
        const hit = specs.find((s) => permissionMatches(s, ex));
        if (hit) failures.push(`${r.id}: ${hit} matches no_match example ${ex}`);
      }
    }
  },
};
