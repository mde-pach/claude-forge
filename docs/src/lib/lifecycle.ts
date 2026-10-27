// What a rule or pack does at each step of a project's life, derived from its
// targets the same way the generator compiles them (src/plan.ts, src/render.ts,
// templates/scripts). Nothing here is configured by hand per rule.
import type { CheckTarget, Pack, Rule, Target, ToolTarget } from "../../../src/types";

export type StepId = "apply" | "session" | "tool" | "edit" | "stop" | "commit" | "ci";
export type Effect = "blocks" | "asks" | "feedback" | "wakes" | "fixes" | "audits" | "informs" | "inactive";

export const STEPS: { id: StepId; label: string; when: string }[] = [
  { id: "apply", label: "forge apply", when: "when you run forge apply or forge status" },
  { id: "session", label: "Session start", when: "when a Claude Code session starts or resumes" },
  { id: "tool", label: "Tool call", when: "before Claude runs a Bash command or reads or edits a file" },
  { id: "edit", label: "After an edit", when: "right after Claude edits or writes a file, or runs a Bash command that changes files" },
  { id: "stop", label: "Claude stops", when: "when Claude ends a turn that changed the tree (Stop hook runs check.sh on what the turn changed)" },
  { id: "commit", label: "git commit", when: "in the pre-commit hook (check.sh --changed)" },
  { id: "ci", label: "CI", when: "in CI (check.sh --since <base branch> on a pull request, --all on the default branch)" },
];

export interface LifecycleEvent {
  step: StepId;
  effect: Effect;
  /** One sentence; `code` spans allowed. */
  text: string;
  /** Applies only in some projects, e.g. "without biome". */
  condition?: string;
  /** For tool-call events: which Claude Code mechanism acts. */
  via?: "permission" | "guard" | "sandbox";
}

/** Where a tool target's diagnostics are enforced once the audit passes. */
const TOOL_STAGE: Record<ToolTarget["tool"], { stage: "lint" | "typecheck"; check: string }> = {
  ruff: { stage: "lint", check: "python/lint" },
  biome: { stage: "lint", check: "typescript/format-lint" },
  ty: { stage: "typecheck", check: "python/typecheck" },
  tsc: { stage: "typecheck", check: "typescript/typecheck" },
};

const code = (s: string) => "`" + s + "`";

/** "python file", "file matching `a` or `b`", "file". */
function scopeNoun(rule: Rule): string {
  const inc = (rule.scope.files ?? []).filter((f) => !f.startsWith("!"));
  const exc = (rule.scope.files ?? []).filter((f) => f.startsWith("!")).map((f) => f.slice(1));
  const base = inc.length ? `file matching ${inc.map(code).join(" or ")}` : rule.scope.language === "any" ? "file" : `${rule.scope.language} file`;
  return exc.length ? `${base} (except ${exc.map(code).join(", ")})` : base;
}

/** Inline command, or a pointer to the code block when it is too long to read inline. */
function cmd(command: string, which: "--all" | "--changed"): string {
  return command.length <= 60 ? code(command) : `the ${code(which)} command below`;
}

function checkEvents(rule: Rule, t: CheckTarget): LifecycleEvent[] {
  if (/\bforge_test_edits\b/.test(t.run))
    return [
      { step: "tool", effect: "asks", text: "An Edit or Write that changes what a test that existed at the start of the turn asserts, or removes it, asks first (nobody being there, an independent reviewer answers, given the turn's request); new tests and setup-only changes are fine." },
      { step: "stop", effect: "blocks", text: "Such a change nobody approved (made through Bash, for instance) blocks Stop; an approved edit does not. Changed test support code is reported once." },
      { step: "commit", effect: "inactive", text: "Not checked: a human edits tests on purpose." },
      { step: "ci", effect: "inactive", text: "Not checked: a human edits tests on purpose." },
    ];
  // advice (§10.7): each finding blocks Stop once per turn, after the failing checks
  const once = `when the turn changed a ${scopeNoun(rule)} and the check finds something, it blocks Stop once per finding, after any failing check; a Stop that finds nothing new passes, so Claude acts on it or says why it does not apply.`;
  if (/^\s*forge_advise\b/.test(t.run))
    return [
      { step: "stop", effect: "blocks", text: `Advisory: ${once}` },
      { step: "commit", effect: "inactive", text: "Not checked: the advice is for Claude's turn." },
      { step: "ci", effect: "inactive", text: "Not checked: the advice is for Claude's turn." },
    ];
  if (/^\s*forge_once\b/.test(t.run))
    return [
      { step: "stop", effect: "blocks", text: `Reported once: ${once}` },
      { step: "commit", effect: "blocks", text: "Fails the commit on any finding." },
      { step: "ci", effect: "blocks", text: "Fails CI on any finding." },
    ];
  const files = t.changed_run?.includes("{files}");
  const changedCmd = t.changed_run ? cmd(t.changed_run, "--changed") : cmd(t.run, "--all");
  const trigger = t.when_changed ? `when ${t.when_changed.map(code).join(" or ")} changed` : `when the change includes a ${scopeNoun(rule)}`;
  const db = t.requires_local_db ? "; skipped with a message unless the database is local" : "";
  const timeout = t.timeout ? ` (timeout ${t.timeout} s)` : "";
  const out: LifecycleEvent[] = [];
  const changed = (effect: Effect, verb: string): LifecycleEvent[] =>
    (["stop", "commit"] as const).map((step) => ({ step, effect, text: `${verb} ${changedCmd} ${trigger}${db}${timeout}.` }));
  switch (t.stage) {
    case "format":
      if (files) out.push({ step: "edit", effect: "fixes", text: `Formats the edited file silently with ${cmd(t.changed_run!.replace("{files}", "<file>"), "--changed")}.` });
      out.push(...changed("fixes", "Runs"));
      out.push({ step: "ci", effect: "blocks", text: `Fails if ${cmd(t.run, "--all")} reports unformatted files.` });
      break;
    case "lint":
      if (files) out.push({ step: "edit", effect: "feedback", text: `Lints the edited file with ${cmd(t.changed_run!.replace("{files}", "<file>"), "--changed")}; findings go back to Claude.` });
      if (t.fix_run) out.push({ step: "edit", effect: "fixes", text: "First applies the safe fixes below to the edited file, and tells Claude when the file changed." });
      else if (files && /--write|--fix/.test(t.changed_run!)) out.push({ step: "edit", effect: "fixes", text: "The same run applies the tool's safe fixes, and tells Claude when they changed the file beyond formatting." });
      out.push(...changed("blocks", "Runs"));
      for (const e of out) if (e.step === "stop" || e.step === "commit") e.text += " Only findings on added lines count.";
      out.push({ step: "ci", effect: "blocks", text: `Runs ${cmd(t.run, "--all")}: with \`--since\`, findings on the lines the branch added count; with \`--all\`, any finding.` });
      break;
    case "typecheck":
      out.push({ step: "edit", effect: "wakes", text: `Runs ${cmd(t.run, "--all")} in the background after an edit to a ${scopeNoun(rule)}; errors in changed files wake Claude, errors only in other files stay silent. Under \`claude -p\` with a prompt, where Claude Code would wait for it, Stop runs it instead.` });
      out.push(...changed("blocks", "Runs"));
      for (const e of out) if (e.step === "stop" || e.step === "commit") e.text += " Only errors in changed files count.";
      out.push({ step: "ci", effect: "blocks", text: `Runs ${cmd(t.run, "--all")}; any error fails.` });
      break;
    case "migration":
    case "test":
      out.push(...changed("blocks", "Runs"));
      out.push({ step: "ci", effect: "blocks", text: `Runs ${cmd(t.run, "--all")}${t.requires_local_db ? " when the database is local" : ""}${timeout}.` });
      break;
  }
  return out;
}

function targetEvents(rule: Rule, t: Target): LifecycleEvent[] {
  const toolTargets = rule.enforce.filter((x): x is ToolTarget => x.target === "tool");
  switch (t.target) {
    case "tool": {
      const s = TOOL_STAGE[t.tool];
      const out: LifecycleEvent[] = [
        { step: "apply", effect: "audits", text: `Checks that your own ${t.tool} config reports ${code(t.expects)} as an error that fails ${t.tool} on the examples below (a warning is not enough); if not, prints the config snippet to add.` },
      ];
      if (s.stage === "lint") out.push({ step: "edit", effect: "feedback", text: `Your ${t.tool}, run on the edited file by ${code(s.check)}, reports ${code(t.expects)} to Claude.` });
      else out.push({ step: "edit", effect: "wakes", text: `Your ${t.tool}, run in the background by ${code(s.check)}, reports ${code(t.expects)} and wakes Claude.` });
      for (const step of ["stop", "commit", "ci"] as const) out.push({ step, effect: "blocks", text: `${code(s.check)} fails on ${code(t.expects)}.` });
      return out.map((e) => ({ ...e, condition: `with ${t.tool}` }));
    }
    case "ast-grep":
    case "grep": {
      const condition = t.target === "ast-grep" && toolTargets.length ? `without ${toolTargets.map((x) => x.tool).join(" or ")}` : undefined;
      const scope = `a changed ${scopeNoun(rule)}`;
      if (t.mode === "advise")
        return [{ step: "edit" as const, effect: "feedback" as const, text: `Matches on lines added during the turn go back to Claude with ${t.target === "grep" ? "file:line" : "file:line:column"}, as advice: Stop, commits and CI do not fail on them.`, condition }];
      if (t.mode === "forbid")
        return [
          { step: "edit", effect: "feedback", text: "Matches on lines added during the turn go back to Claude with file:line:column." },
          { step: "stop", effect: "blocks", text: `A match on a line the turn added to ${scope} blocks Stop.` },
          { step: "commit", effect: "blocks", text: `A match on a line added since \`HEAD\` to ${scope} fails the commit.` },
          { step: "ci", effect: "blocks", text: "With `--since`, a match on a line the branch added fails CI; with `--all`, any match anywhere in scope." },
        ].map((e) => ({ ...e, step: e.step as StepId, effect: e.effect as Effect, condition }));
      const more = t.mode === "no-increase" ? "more" : "fewer";
      return [
        { step: "stop" as const, effect: "blocks" as const, text: `${scope.replace(/^a /, "A ")} with ${more} matches than at the start of the turn blocks Stop.`, condition },
        { step: "commit" as const, effect: "blocks" as const, text: `${scope.replace(/^a /, "A ")} with ${more} matches than at \`HEAD\` fails the commit.`, condition },
        { step: "ci" as const, effect: "blocks" as const, text: `With \`--since\`, ${scope.replace(/^a /, "")} with ${more} matches than at the merge base fails CI (\`--all\` has nothing to compare).`, condition },
      ];
    }
    case "permission":
      return [
        {
          step: "tool",
          via: "permission",
          effect: t.decision === "deny" ? "blocks" : "asks",
          text: `Claude Code ${t.decision === "deny" ? "refuses" : "asks you before"} ${code(t.rule)} (permission ${t.decision}).`,
        },
      ];
    case "guard":
      return [
        {
          step: "tool",
          via: "guard",
          effect: t.decision === "deny" ? "blocks" : "asks",
          text: t.check
            ? `The PreToolUse guard ${t.decision === "deny" ? "denies" : "asks you before"} a Bash command its ${code(t.check)} check flags (the pattern below only pre-filters).`
            : `The PreToolUse guard ${t.decision === "deny" ? "denies" : "asks you before"} any Bash command matching the pattern below.`,
        },
      ];
    case "sandbox":
      return [{ step: "tool", via: "sandbox", effect: "blocks", text: `Bash runs in Claude Code's sandbox with ${code(`sandbox.${t.path}`)} = ${code(JSON.stringify(t.value))}.` }];
    case "check":
      return checkEvents(rule, t);
  }
}

export function ruleLifecycle(rule: Rule): LifecycleEvent[] {
  const events: LifecycleEvent[] = [];
  // permission targets: one event per decision, listing every permission string
  for (const decision of ["deny", "ask"] as const) {
    const specs = rule.enforce.filter((t) => t.target === "permission" && t.decision === decision).map((t) => (t as { rule: string }).rule);
    if (specs.length)
      events.push({
        step: "tool",
        via: "permission",
        effect: decision === "deny" ? "blocks" : "asks",
        text: `Claude Code ${decision === "deny" ? "refuses" : "asks you before"} ${specs.map(code).join(", ")} (permission ${decision}).`,
      });
  }
  const guards = new Set<string>();
  for (const t of rule.enforce) {
    if (t.target === "permission") continue;
    if (t.target === "guard") {
      if (guards.has(t.decision)) continue;
      guards.add(t.decision);
    }
    events.push(...targetEvents(rule, t));
  }
  return events;
}

/** Per step: the pack's own context, and which of its rules act there. */
export function packLifecycle(pack: Pack): { step: StepId; context: boolean; rules: { id: string; effects: Effect[] }[] }[] {
  return STEPS.map(({ id }) => ({
    step: id,
    context: id === "session" && pack.context !== undefined,
    rules: pack.rules
      .map((r) => ({ id: r.id, effects: [...new Set(ruleLifecycle(r).filter((e) => e.step === id && e.effect !== "inactive").map((e) => e.effect))] }))
      .filter((r) => r.effects.length > 0),
  }));
}
