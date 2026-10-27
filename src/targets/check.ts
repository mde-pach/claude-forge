/** `check`: a command check.sh runs in a stage (format, lint, codegen, typecheck, migration, test), §10. */
import { checkTools, directDashedTools } from "../core/tools.js";
import { checkExamplesOptional, type CheckExampleRun, hadolintIntegrity, runCheckExamples } from "../selftest/runners.js";
import type { CheckTarget, Rule } from "../types.js";
import { cmd, code, type Effect, type LifecycleEvent, scopeNoun } from "./events.js";
import type { TargetKind } from "./kind.js";

export const STAGES = ["format", "lint", "codegen", "typecheck", "migration", "test"] as const;

export const check: TargetKind<CheckTarget> = {
  kind: "check",
  phase: 2,
  schema: {
    additionalProperties: false,
    required: ["stage", "run"],
    properties: {
      target: true,
      stage: { enum: [...STAGES] },
      run: { type: "string", minLength: 1 },
      changed_run: { type: "string", minLength: 1 },
      fix_run: { type: "string", minLength: 1, pattern: "\\{files\\}" },
      format_stdin: { type: "string", minLength: 1, pattern: "\\{file\\}" },
      rerun: { type: "string", minLength: 1, pattern: "\\{files\\}" },
      when_changed: { type: "array", items: { type: "string", minLength: 1 }, minItems: 1 },
      // lint only: the tool's config files it also lints (ruff's RUF200 on pyproject.toml), in the check's scope
      config_files: { type: "array", items: { type: "string", minLength: 1 }, minItems: 1 },
      requires_local_db: { type: "boolean" },
      // the check's warnings block like errors (the run fails on them: oxlint --deny-warnings)
      fail_on_warnings: { const: true },
      timeout: { type: "integer", minimum: 1, maximum: 600 },
    },
    allOf: [
      // rerun re-runs failing tests (§10.6); format_stdin formats base copies (§10.1)
      { if: { required: ["rerun"] }, then: { properties: { stage: { const: "test" } } } },
      { if: { required: ["format_stdin"] }, then: { properties: { stage: { enum: ["format", "lint"] } } } },
      { if: { required: ["config_files"] }, then: { properties: { stage: { const: "lint" } } } },
    ],
  },
  label: (t) => `check · ${t.stage}`,
  validate(_rule, t) {
    for (const command of [t.run, t.changed_run, t.fix_run, t.format_stdin, t.rerun])
      for (const tool of directDashedTools(command ?? ""))
        throw new Error(`invalid rule: run ${tool} as "forge_exec_tool ${tool}" (a name with "-" has no wrapper function, so it would not be the project's own)`);
  },
  needsExamples: (t) => !checkExamplesOptional(t),
  plan(ctx, rule, st, t) {
    if (ctx.level === "user") {
      ctx.warnings.push(`${rule.id}: check targets are project-level only`);
      return;
    }
    const { dirs, absent } = ctx.checkPlace(rule, t);
    if (absent.length) st.notEnforced.push(`not enforced here: ${absent.join(", ")} not configured`);
    else {
      ctx.out.checks.push({ rule, target: t, ...(dirs ? { dirs } : {}) });
      st.enforced.push(`check ${t.stage}: ${t.run}${dirs ? ` (in ${dirs.join(", ")})` : ""}`);
    }
  },
  enforcement(st, t, missing) {
    if (!st.enforced.some((e) => e.startsWith("check "))) return {};
    const gone = checkTools(t).filter((x) => missing.has(x));
    return gone.length ? { not: `${gone.join(", ")} not installed` } : { by: `check ${t.stage}` };
  },
  lifecycle: (rule, t) => checkEvents(rule, t),
  // each example in its own temporary repository, several at a time
  selftest(rules, { report, toolBins }) {
    const { failures, notes, counts } = report;
    const checkRules = rules.filter((r) => r.examples?.match?.length || r.examples?.no_match?.length);
    counts.check = checkRules.length;
    const runs: (CheckExampleRun & { n: number })[] = [];
    for (const r of checkRules)
      for (const t of r.enforce.filter((x): x is CheckTarget => x.target === "check"))
        for (const kind of ["match", "no_match"] as const)
          (r.examples?.[kind] ?? []).forEach((ex, i) => {
            runs.push({ rule: r, target: t, example: ex, kind, n: i + 1 });
          });
    runCheckExamples(runs, toolBins).forEach((res, j) => {
      const { rule: r, kind, n } = runs[j]!;
      if (res.outcome === "fail") failures.push(`${r.id}: ${kind} example ${n} ${res.detail}`);
      else if (res.outcome === "skip") notes.push(`${r.id}: ${kind} example ${n} skipped: ${res.detail}`);
    });
    // hadolint: the hadolint-bin wheel's binary is the upstream release binary (§13.3)
    if (rules.some((r) => r.enforce.some((t) => t.target === "check" && /\bforge_hadolint\b/.test(t.run)))) {
      const h = hadolintIntegrity();
      if (h.status === "mismatch") failures.push(`hadolint: ${h.detail}`);
      else if (h.status === "skipped") notes.push(`hadolint: ${h.detail}`);
    }
  },
};

function checkEvents(rule: Rule, t: CheckTarget): LifecycleEvent[] {
  if (/\bforge_test_edits\b/.test(t.run))
    return [
      {
        step: "tool",
        effect: "asks",
        text: "An Edit or Write that changes what a test that existed at the start of the turn asserts, or removes it, asks first (nobody being there, an independent reviewer answers, given the turn's request); new tests and setup-only changes are fine.",
      },
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
  const changed = (effect: Effect, verb: string): LifecycleEvent[] => (["stop", "commit"] as const).map((step) => ({ step, effect, text: `${verb} ${changedCmd} ${trigger}${db}${timeout}.` }));
  switch (t.stage) {
    case "format":
      if (files) out.push({ step: "edit", effect: "fixes", text: `Formats the edited file silently with ${cmd(t.changed_run!.replace("{files}", "<file>"), "--changed")}.` });
      out.push(...changed("fixes", "Runs"));
      out.push({ step: "ci", effect: "blocks", text: `Fails if ${cmd(t.run, "--all")} reports unformatted files.` });
      break;
    case "lint":
      if (files) out.push({ step: "edit", effect: "feedback", text: `Lints the edited file with ${cmd(t.changed_run!.replace("{files}", "<file>"), "--changed")}; findings go back to Claude.` });
      if (t.fix_run) out.push({ step: "edit", effect: "fixes", text: "First applies the safe fixes below to the edited file, and tells Claude when the file changed." });
      else if (files && /--write|--fix/.test(t.changed_run!))
        out.push({ step: "edit", effect: "fixes", text: "The same run applies the tool's safe fixes, and tells Claude when they changed the file beyond formatting." });
      out.push(...changed("blocks", "Runs"));
      for (const e of out) if (e.step === "stop" || e.step === "commit") e.text += " Only findings on added lines count.";
      out.push({ step: "ci", effect: "blocks", text: `Runs ${cmd(t.run, "--all")}: with \`--since\`, findings on the lines the branch added count; with \`--all\`, any finding.` });
      break;
    case "typecheck":
      out.push({
        step: "edit",
        effect: "wakes",
        text: `Runs ${cmd(t.run, "--all")} in the background after an edit to a ${scopeNoun(rule)}; errors in changed files wake Claude, errors only in other files stay silent. Under \`claude -p\` with a prompt, where Claude Code would wait for it, Stop runs it instead.`,
      });
      out.push(...changed("blocks", "Runs"));
      for (const e of out) if (e.step === "stop" || e.step === "commit") e.text += " Only errors in changed files count.";
      out.push({ step: "ci", effect: "blocks", text: `Runs ${cmd(t.run, "--all")}; any error fails.` });
      break;
    case "codegen":
      // between lint and the typecheck; a stale generated file stops check.sh before the typecheck (§10.1)
      out.push(...changed("blocks", "Runs"));
      for (const e of out) if (e.step === "stop" || e.step === "commit") e.text += " A failure stops the check there, before the typecheck.";
      out.push({ step: "ci", effect: "blocks", text: `Runs ${cmd(t.run, "--all")}${timeout}.` });
      break;
    case "migration":
    case "test":
      out.push(...changed("blocks", "Runs"));
      out.push({ step: "ci", effect: "blocks", text: `Runs ${cmd(t.run, "--all")}${t.requires_local_db ? " when the database is local" : ""}${timeout}.` });
      break;
  }
  return out;
}
