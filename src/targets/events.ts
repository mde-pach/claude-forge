/**
 * What a rule does at each step of a project's life (the docs' lifecycle pages): the steps,
 * the effects, and the sentence helpers the target kinds describe their events with.
 */
import type { AstGrepMode, Rule } from "../types.js";

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

export const code = (s: string) => "`" + s + "`";

/** "python file", "file matching `a` or `b`", "file". */
export function scopeNoun(rule: Rule): string {
  const inc = (rule.scope.files ?? []).filter((f) => !f.startsWith("!"));
  const exc = (rule.scope.files ?? []).filter((f) => f.startsWith("!")).map((f) => f.slice(1));
  const base = inc.length ? `file matching ${inc.map(code).join(" or ")}` : rule.scope.language === "any" ? "file" : `${rule.scope.language} file`;
  return exc.length ? `${base} (except ${exc.map(code).join(", ")})` : base;
}

/** Inline command, or a pointer to the code block when it is too long to read inline. */
export function cmd(command: string, which: "--all" | "--changed"): string {
  return command.length <= 60 ? code(command) : `the ${code(which)} command below`;
}

/** The events of a pattern target (ast-grep, grep) by mode; `condition` when it applies only in some projects. */
export function patternEvents(rule: Rule, kind: "ast-grep" | "grep", mode: AstGrepMode, condition?: string): LifecycleEvent[] {
  const scope = `a changed ${scopeNoun(rule)}`;
  if (mode === "advise")
    return [
      {
        step: "edit" as const,
        effect: "feedback" as const,
        text: `Matches on lines added during the turn go back to Claude with ${kind === "grep" ? "file:line" : "file:line:column"}, as advice: Stop, commits and CI do not fail on them.`,
        condition,
      },
    ];
  if (mode === "forbid")
    return [
      { step: "edit", effect: "feedback", text: "Matches on lines added during the turn go back to Claude with file:line:column." },
      { step: "stop", effect: "blocks", text: `A match on a line the turn added to ${scope} blocks Stop.` },
      { step: "commit", effect: "blocks", text: `A match on a line added since \`HEAD\` to ${scope} fails the commit.` },
      { step: "ci", effect: "blocks", text: "With `--since`, a match on a line the branch added fails CI; with `--all`, any match anywhere in scope." },
    ].map((e) => ({ ...e, step: e.step as StepId, effect: e.effect as Effect, condition }));
  const more = mode === "no-increase" ? "more" : "fewer";
  return [
    { step: "stop" as const, effect: "blocks" as const, text: `${scope.replace(/^a /, "A ")} with ${more} matches than at the start of the turn blocks Stop.`, condition },
    { step: "commit" as const, effect: "blocks" as const, text: `${scope.replace(/^a /, "A ")} with ${more} matches than at \`HEAD\` fails the commit.`, condition },
    {
      step: "ci" as const,
      effect: "blocks" as const,
      text: `With \`--since\`, ${scope.replace(/^a /, "")} with ${more} matches than at the merge base fails CI (\`--all\` has nothing to compare).`,
      condition,
    },
  ];
}
