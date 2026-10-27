/**
 * The target kinds (§6.2), one module each. TARGETS is the one place that lists them: the
 * rule schema, the plan, forge status, the self-test and the docs' lifecycle pages iterate it.
 */
import type { Pack, Rule, Target } from "../types.js";
import { astGrep } from "./ast-grep.js";
import { check } from "./check.js";
import { type Effect, type LifecycleEvent, STEPS, type StepId } from "./events.js";
import { grep } from "./grep.js";
import { guard } from "./guard.js";
import type { TargetKind } from "./kind.js";
import { permission } from "./permission.js";
import { sandbox } from "./sandbox.js";
import { tool } from "./tool.js";

type KindOf<K extends Target["target"]> = TargetKind<Extract<Target, { target: K }>>;

/** Every kind, in the order the schema lists them and the self-test runs them. */
export const TARGETS: { [K in Target["target"]]: KindOf<K> } = {
  tool,
  "ast-grep": astGrep,
  permission,
  guard,
  sandbox,
  check,
  grep,
};

export const TARGET_KINDS = Object.keys(TARGETS) as Target["target"][];

/** The kind of a target. */
export function kindOf<T extends Target>(t: T): TargetKind<T> {
  return TARGETS[t.target] as unknown as TargetKind<T>;
}

/** The rule's targets in the order the plan handles them: phase 0, then 1, then 2, each in the rule's order. */
export function targetsByPhase(rule: Pick<Rule, "enforce">): Target[] {
  return ([0, 1, 2] as const).flatMap((phase) => rule.enforce.filter((t) => kindOf(t).phase === phase));
}

/** The docs' short labels of what enforces the rule ("guard deny", "check · lint"), each once. */
export function enforcedBy(rule: Rule): string[] {
  return [...new Set(rule.enforce.map((t) => kindOf(t).label(t)))];
}

// ---------------------------------------------------------------- lifecycle (the docs)

export { STEPS, type Effect, type LifecycleEvent, type StepId };

/** What the rule does at each step: each kind's lead events first, then each target's in the rule's order. */
export function ruleLifecycle(rule: Rule): LifecycleEvent[] {
  const ofKind = (k: Target["target"]) => rule.enforce.filter((t) => t.target === k);
  const events: LifecycleEvent[] = [];
  for (const k of TARGET_KINDS) {
    const lead = (TARGETS[k] as TargetKind).lifecycleLead;
    const mine = ofKind(k);
    if (lead && mine.length) events.push(...lead(rule, mine));
  }
  for (const t of rule.enforce) events.push(...kindOf(t).lifecycle(rule, t, ofKind(t.target)));
  return events;
}

/** Per step: the pack's own context, and which of its rules act there. */
export function packLifecycle(pack: Pack): { step: StepId; context: boolean; rules: { id: string; effects: Effect[] }[] }[] {
  return STEPS.map(({ id }) => ({
    step: id,
    context: id === "session" && pack.context !== undefined,
    rules: pack.rules
      .map((r) => ({
        id: r.id,
        effects: [
          ...new Set(
            ruleLifecycle(r)
              .filter((e) => e.step === id && e.effect !== "inactive")
              .map((e) => e.effect),
          ),
        ],
      }))
      .filter((r) => r.effects.length > 0),
  }));
}
