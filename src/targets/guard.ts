/** `guard`: a PreToolUse tripwire on Bash commands, deny or ask, in guard.rules (§9.2). */
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { guardExamples } from "../examples.js";
import { renderGuardRules, template } from "../render.js";
import { availableShells, guardExampleDir, guardJob, guardOutcome } from "../selftest/runners.js";
import type { GuardTarget } from "../types.js";
import { type Job, runParallel } from "../util.js";
import { code } from "./events.js";
import type { TargetKind } from "./kind.js";

/** Built-in guard checks: guard_check_<name> in guard.sh decides on the parsed command. */
export const GUARD_CHECKS = ["ephemeral-exec"] as const;

export const guard: TargetKind<GuardTarget> = {
  kind: "guard",
  phase: 2,
  schema: {
    additionalProperties: false,
    required: ["decision", "pattern"],
    properties: {
      target: true,
      decision: { enum: ["deny", "ask"] },
      pattern: { type: "string", minLength: 1, pattern: "^[^\\t\\n]+$" },
      check: { enum: [...GUARD_CHECKS] },
    },
    // a built-in check decides which matching commands to ask about: a deny would refuse them all
    if: { required: ["check"] },
    then: { properties: { decision: { const: "ask" } } },
  },
  label: (t) => `guard ${t.decision}`,
  needsExamples: () => true,
  plan(ctx, rule, st, t) {
    ctx.out.guards.push({ rule, target: t });
    if (!st.enforced.some((e) => e.startsWith("guard"))) st.enforced.push(`guard ${t.decision} (PreToolUse Bash)`);
  },
  enforcement: (_st, t) => ({ by: `guard ${t.decision}` }),
  lifecycle(_rule, t, siblings) {
    // one event per decision
    if (siblings.find((s) => s.decision === t.decision) !== t) return [];
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
  },
  // each rule in its own directory, every example on every available shell, several at a time
  selftest(rules, { report }) {
    const { failures, notes, counts } = report;
    counts.guard = rules.length;
    const shells = availableShells();
    for (const s of ["sh", "dash", "bash --posix", "busybox sh"]) if (!shells.some((x) => x.name === s)) notes.push(`guard: ${s} not available, skipped`);
    const dirs: string[] = [];
    const runs: { job: Job; report: (got: string) => void }[] = [];
    try {
      for (const r of rules) {
        const dir = mkdtempSync(join(tmpdir(), "forge-guard-"));
        dirs.push(dir);
        writeFileSync(join(dir, "guard.sh"), template("guard.sh"));
        const targets = r.enforce.filter((t): t is GuardTarget => t.target === "guard");
        writeFileSync(
          join(dir, "guard.rules"),
          renderGuardRules(
            targets.map((target) => ({ rule: r, target })),
            "/",
          ),
        );
        const decision = targets.some((t) => t.decision === "deny") ? "deny" : "ask";
        for (const kind of ["match", "no_match"] as const)
          for (const ex of guardExamples(r, kind)) {
            const cwd = guardExampleDir(ex.files);
            dirs.push(cwd);
            for (const sh of shells) {
              const want = kind === "match" ? decision : "";
              runs.push({
                job: guardJob(sh.argv, dir, ex.command, cwd),
                report: (got) => {
                  if (got !== want)
                    failures.push(
                      `${r.id} [${sh.name}]: expected ${want || "nothing"} for ${JSON.stringify(ex.command)}${ex.files ? ` with ${Object.keys(ex.files).join(", ")}` : ""}, got ${got || "nothing"}`,
                    );
                },
              });
            }
          }
      }
      runParallel(runs.map((x) => x.job)).forEach((res, i) => {
        runs[i]!.report(guardOutcome(res));
      });
    } finally {
      for (const d of dirs) rmSync(d, { recursive: true, force: true });
    }
  },
};
