/**
 * A target kind: everything Forge does with one kind of `enforce` entry (§6.2), in one
 * module. Adding a kind is adding a module that implements this interface and listing it
 * in ./index.ts; the schema, the plan, `forge status`, the self-test and the docs'
 * lifecycle pages follow from it.
 */
import type { ActiveAstGrep, ActiveCheck, ActiveGrep, ActiveGuard } from "../render/index.js";
import type { AstGrepTarget, CheckTarget, DesiredEntry, Level, Rule, RuleFile, RuleStatus, SwitchedOff, Target, TestReport, ToolName, ToolTarget } from "../types.js";
import type { LifecycleEvent } from "./events.js";

/** What a target's plan step reads from the plan, and where it adds what it enforces. */
export interface PlanContext {
  level: Level;
  /** Where permission and sandbox entries go. */
  settingsFile: string;
  warnings: string[];
  out: {
    settings: DesiredEntry[];
    guards: ActiveGuard[];
    greps: ActiveGrep[];
    checks: ActiveCheck[];
    astgrep: ActiveAstGrep[];
  };
  /** Whether an audited tool is configured here (§11). */
  present(tool: ToolName): boolean;
  /** The active pack's switch-off of this tool diagnostic (pack.yaml tool_off), if any. */
  switchedOff(t: ToolTarget): SwitchedOff | undefined;
  /** The workspace packages the rule applies in (undefined: the whole repository). */
  ruleDirs(rule: Rule): string[] | undefined;
  /** The packages the types engine needs for this target that are not declared where the rule applies. */
  missingNeeds(rule: Rule, t: AstGrepTarget): string[];
  /** Where a check runs (dirs: workspace packages) or which of its audited tools are absent. */
  checkPlace(rule: Rule, t: CheckTarget): { dirs?: string[]; absent: string[] };
}

/** What a target kind's self-test reads, and where it reports. */
export interface SelftestContext {
  report: TestReport;
  /** The project whose local rules are tested (forge test), if any. */
  repo?: string;
  /** Directories holding the project tools check examples run. */
  toolBins: string[];
}

export interface TargetKind<T extends Target = Target> {
  kind: T["target"];
  /**
   * When the plan handles it: 0 and 1 before the others, so that an ast-grep target (1)
   * stands in for a tool target (0) whose tool is absent; 2 in the rule's own order.
   */
  phase: 0 | 1 | 2;
  /** The JSON Schema of the target's fields (the `then` of its `if: {target: <kind>}`). */
  schema: Record<string, unknown>;
  /** Short name on the docs' rule cards: "guard deny", "check · lint". */
  label(t: T): string;
  /** Checks a rule file's target when packs and local rules load (beyond the schema); throws on an invalid one. */
  validate?(rule: RuleFile, t: T): void;
  /** Whether a rule with this target needs match and no_match examples. */
  needsExamples(t: T): boolean;
  /** Adds what the target enforces here to the plan, and says where in `st`. */
  plan(ctx: PlanContext, rule: Rule, st: RuleStatus, t: T): void;
  /** forge status: what enforces the rule here (`by`), or why this target does not (`not`). */
  enforcement(st: RuleStatus, t: T, missingTools: Set<string>): { by?: string; not?: string };
  /** The docs' lifecycle events of one target; `siblings` are the rule's targets of this kind. */
  lifecycle(rule: Rule, t: T, siblings: T[]): LifecycleEvent[];
  /** Events of all the rule's targets of this kind, shown before every other event of the rule. */
  lifecycleLead?(rule: Rule, targets: T[]): LifecycleEvent[];
  /** Runs the examples of the rules having this kind of target. */
  selftest?(rules: Rule[], ctx: SelftestContext): void;
}
