// What a rule or pack does at each step of a project's life, from its targets as the Rust loader
// dumps them (claude-forge packs dump --json). One describer per target kind.
import type { CheckTarget, Mode, Pack, Rule, Target } from "./types";

export type StepId = "apply" | "session" | "tool" | "edit" | "stop" | "commit" | "ci";
export type Effect = "blocks" | "asks" | "feedback" | "wakes" | "fixes" | "audits" | "informs" | "inactive";

export const STEPS: { id: StepId; label: string; when: string }[] = [
  { id: "apply", label: "forge apply", when: "when you run forge apply or forge status" },
  { id: "session", label: "Session start", when: "when a Claude Code session starts or resumes" },
  { id: "tool", label: "Tool call", when: "before Claude runs a Bash command or reads or edits a file" },
  { id: "edit", label: "After an edit", when: "right after Claude edits or writes a file, or runs a Bash command that changes files" },
  { id: "stop", label: "Claude stops", when: "when Claude ends a turn that changed the tree (the Stop hook checks what the turn changed)" },
  { id: "commit", label: "git commit", when: "in the pre-commit hook (forge check --changed)" },
  { id: "ci", label: "CI", when: "in CI (forge check --since <base branch> on a pull request, --all on the default branch)" },
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

function patternEvents(rule: Rule, kind: "ast-grep" | "grep", mode: Mode, condition?: string): LifecycleEvent[] {
  const scope = `a changed ${scopeNoun(rule)}`;
  const Scope = scope.replace(/^a /, "A ");
  if (mode === "advise")
    return [
      {
        step: "edit",
        effect: "feedback",
        text: `Matches on lines added during the turn go back to Claude with ${kind === "grep" ? "file:line" : "file:line:column"}, as advice: Stop, commits and CI do not fail on them.`,
        condition,
      },
    ];
  if (mode === "forbid")
    return (
      [
        { step: "edit", effect: "feedback", text: "Matches on lines added during the turn go back to Claude with file:line." },
        { step: "stop", effect: "blocks", text: `A match on a line the turn added to ${scope} blocks Stop.` },
        { step: "commit", effect: "blocks", text: `A match on a line added since \`HEAD\` to ${scope} fails the commit.` },
        { step: "ci", effect: "blocks", text: "With `--since`, a match on a line the branch added fails CI; with `--all`, any match anywhere in scope." },
      ] as LifecycleEvent[]
    ).map((e) => ({ ...e, condition }));
  const more = mode === "no-increase" ? "more" : "fewer";
  return [
    { step: "stop", effect: "blocks", text: `${Scope} with ${more} matches than at the start of the turn blocks Stop.`, condition },
    { step: "commit", effect: "blocks", text: `${Scope} with ${more} matches than at \`HEAD\` fails the commit.`, condition },
    { step: "ci", effect: "blocks", text: `With \`--since\`, ${scope.replace(/^a /, "")} with ${more} matches than at the merge base fails CI (\`--all\` has nothing to compare).`, condition },
  ];
}

const TOOL_STAGE: Record<string, { stage: "lint" | "typecheck"; check: string }> = {
  ruff: { stage: "lint", check: "python/lint" },
  biome: { stage: "lint", check: "typescript/format-lint" },
  ty: { stage: "typecheck", check: "python/typecheck" },
  tsc: { stage: "typecheck", check: "typescript/typecheck" },
  eslint: { stage: "lint", check: "typescript/eslint/lint" },
};

function checkEvents(rule: Rule, t: CheckTarget): LifecycleEvent[] {
  if (/\bforge_test_edits\b/.test(t.run))
    return [
      {
        step: "tool",
        effect: "asks",
        text: "An Edit or Write that changes what a test that existed at the start of the turn asserts, or removes it, asks first (with nobody there, an independent reviewer answers, given the turn's request); new tests and setup-only changes are fine.",
      },
      { step: "stop", effect: "blocks", text: "Such a change nobody approved (made through Bash, for instance) blocks Stop; an approved edit does not." },
      { step: "commit", effect: "inactive", text: "Not checked: a human edits tests on purpose." },
      { step: "ci", effect: "inactive", text: "Not checked: a human edits tests on purpose." },
    ];
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
  const db = t.requires_local_db ? "; skipped with a note unless the database is local" : "";
  const timeout = t.timeout ? ` (timeout ${t.timeout} s)` : "";
  const out: LifecycleEvent[] = [];
  const changed = (effect: Effect, extra = ""): LifecycleEvent[] => (["stop", "commit"] as const).map((step) => ({ step, effect, text: `Runs ${changedCmd} ${trigger}${db}${timeout}.${extra}` }));
  switch (t.stage) {
    case "format":
      if (files) out.push({ step: "edit", effect: "fixes", text: `Formats the edited file silently with ${cmd(t.changed_run!.replace("{files}", "<file>"), "--changed")}.` });
      out.push(...changed("fixes"));
      out.push({ step: "ci", effect: "blocks", text: `Fails if ${cmd(t.run, "--all")} reports unformatted files.` });
      break;
    case "lint":
      if (files) out.push({ step: "edit", effect: "feedback", text: `Lints the edited file with ${cmd(t.changed_run!.replace("{files}", "<file>"), "--changed")}; findings go back to Claude.` });
      if (t.fix_run) out.push({ step: "edit", effect: "fixes", text: "First applies the safe fixes below to the edited file, and tells Claude when the file changed." });
      out.push(...changed("blocks", " Only findings on added lines count."));
      out.push({ step: "ci", effect: "blocks", text: `Runs ${cmd(t.run, "--all")}: with \`--since\`, findings on the lines the branch added count; with \`--all\`, any finding.` });
      break;
    case "typecheck":
      out.push({
        step: "edit",
        effect: "wakes",
        text: `Runs in the background after an edit to a ${scopeNoun(rule)}; errors in changed files wake Claude, errors only in other files stay silent.`,
      });
      out.push(...changed("blocks", " Only errors in changed files count."));
      out.push({ step: "ci", effect: "blocks", text: `Runs ${cmd(t.run, "--all")}; any error fails.` });
      break;
    case "codegen":
      out.push(...changed("blocks", " A failure stops the check there, before the typecheck."));
      out.push({ step: "ci", effect: "blocks", text: `Runs ${cmd(t.run, "--all")}${timeout}.` });
      break;
    default:
      out.push(...changed("blocks"));
      out.push({ step: "ci", effect: "blocks", text: `Runs ${cmd(t.run, "--all")}${t.requires_local_db ? " when the database is local" : ""}${timeout}.` });
  }
  return out;
}

function targetEvents(rule: Rule, t: Target, siblings: Target[]): LifecycleEvent[] {
  switch (t.target) {
    case "tool": {
      const s = TOOL_STAGE[t.tool] ?? { stage: "lint", check: t.tool };
      const out: LifecycleEvent[] = [
        {
          step: "apply",
          effect: "audits",
          text: `Checks that your own ${t.tool} config reports ${code(t.expects)} as an error (a warning is not enough); if not, forge status prints the config snippet to add.`,
        },
        s.stage === "lint"
          ? { step: "edit", effect: "feedback", text: `Your ${t.tool}, run on the edited file by ${code(s.check)}, reports ${code(t.expects)} to Claude.` }
          : { step: "edit", effect: "wakes", text: `Your ${t.tool}, run in the background by ${code(s.check)}, reports ${code(t.expects)} and wakes Claude.` },
        ...(["stop", "commit", "ci"] as const).map((step) => ({ step, effect: "blocks" as const, text: `${code(s.check)} fails on ${code(t.expects)}.` })),
      ];
      return out.map((e) => ({ ...e, condition: `with ${t.tool}` }));
    }
    case "ast-grep": {
      const tools = rule.enforce.filter((x) => x.target === "tool").map((x) => (x as { tool: string }).tool);
      return patternEvents(rule, "ast-grep", t.mode, tools.length ? `without ${tools.join(" or ")}` : undefined);
    }
    case "grep":
      return patternEvents(rule, "grep", t.mode);
    case "guard":
      // one event per decision
      if (siblings.find((s) => s.target === "guard" && s.decision === t.decision) !== t) return [];
      return [
        {
          step: "tool",
          via: "guard",
          effect: t.decision === "deny" ? "blocks" : "asks",
          text: t.check
            ? `The PreToolUse hook ${t.decision === "deny" ? "denies" : "asks you before"} a Bash command its ${code(t.check)} check flags.`
            : `The PreToolUse hook ${t.decision === "deny" ? "denies" : "asks you before"} a Bash command whose parsed simple commands match the pattern below (quoted arguments and heredoc bodies are data, not commands).`,
        },
      ];
    case "sandbox":
      return [{ step: "tool", via: "sandbox", effect: "blocks", text: `Bash runs in Claude Code's sandbox with ${code(`sandbox.${t.path}`)} = ${code(JSON.stringify(t.value))}.` }];
    case "check":
      return checkEvents(rule, t);
    case "permission":
      return [];
  }
}

/** What the rule does at each step: its permissions first (one event per decision), then each target's in the rule's order. */
export function ruleLifecycle(rule: Rule): LifecycleEvent[] {
  const events: LifecycleEvent[] = [];
  for (const decision of ["deny", "ask"] as const) {
    const specs = rule.enforce.flatMap((t) => (t.target === "permission" && t.decision === decision ? [t.rule] : []));
    if (specs.length)
      events.push({
        step: "tool",
        via: "permission",
        effect: decision === "deny" ? "blocks" : "asks",
        text: `Claude Code ${decision === "deny" ? "refuses" : "asks you before"} ${specs.map(code).join(", ")} (permission ${decision}).`,
      });
  }
  for (const t of rule.enforce) events.push(...targetEvents(rule, t, rule.enforce));
  return events;
}

/** The pack's context items, as session events. */
export function contextEvents(pack: Pack): LifecycleEvent[] {
  return (pack.context ?? []).length ? [{ step: "session", effect: "informs", text: "the pack's context is in a rules file Claude reads" }] : [];
}

/** A target's short label on rule cards ("guard deny", "check · lint"). */
export function targetLabel(t: Target): string {
  switch (t.target) {
    case "tool":
      return `${t.tool} audit`;
    case "ast-grep":
    case "grep":
      return `${t.target} ${t.mode}`;
    case "permission":
    case "guard":
      return `${t.target} ${t.decision}`;
    case "sandbox":
      return "sandbox";
    case "check":
      return `check · ${t.stage}`;
  }
}

/** The labels of what enforces the rule, each once. */
export function enforcedBy(rule: Rule): string[] {
  return [...new Set(rule.enforce.map(targetLabel))];
}
