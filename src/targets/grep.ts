/** `grep`: a POSIX ERE matched line by line, for text ast-grep does not parse (comments, config, .astro files). */
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { codeExamples } from "../examples.js";
import type { GrepTarget } from "../types.js";
import { run } from "../util.js";
import { MODES } from "./ast-grep.js";
import { patternEvents } from "./events.js";
import type { TargetKind } from "./kind.js";

export const grep: TargetKind<GrepTarget> = {
  kind: "grep",
  phase: 2,
  schema: {
    additionalProperties: false,
    required: ["mode", "pattern"],
    properties: {
      target: true,
      mode: { enum: [...MODES] },
      pattern: { type: "string", minLength: 1, pattern: "^[^\\t\\n]+$" },
      ignore_case: { type: "boolean" },
    },
  },
  label: (t) => `grep ${t.mode}`,
  needsExamples: () => true,
  plan(ctx, rule, st, t) {
    if (ctx.level === "user") {
      ctx.warnings.push(`${rule.id}: grep targets are project-level only`);
      return;
    }
    const dirs = ctx.ruleDirs(rule);
    ctx.out.greps.push({ rule, target: t, ...(dirs ? { dirs } : {}) });
    st.enforced.push(`grep ${t.mode} (.forge/check.sh)${dirs ? ` (in ${dirs.join(", ")})` : ""}`);
  },
  enforcement: () => ({ by: "grep" }),
  lifecycle: (rule, t) => patternEvents(rule, "grep", t.mode),
  // the pattern (grep -E) matches a line of every match example and none of a no_match example
  selftest(rules, { report }) {
    const { failures, counts } = report;
    counts.grep = rules.length;
    const dir = mkdtempSync(join(tmpdir(), "forge-grep-"));
    try {
      for (const r of rules)
        for (const t of r.enforce.filter((x): x is GrepTarget => x.target === "grep"))
          for (const kind of ["match", "no_match"] as const)
            codeExamples(r, kind).forEach((ex, i) => {
              writeFileSync(join(dir, "example"), ex);
              const n = Number(run("sh", ["-c", 'grep -c "$3" -- "$1" "$2"', "sh", t.pattern, join(dir, "example"), t.ignore_case ? "-iE" : "-E"]).stdout.trim() || "0");
              if (kind === "match" && n === 0) failures.push(`${r.id}: grep pattern does not match match example ${i + 1}: ${JSON.stringify(ex)}`);
              if (kind === "no_match" && n > 0) failures.push(`${r.id}: grep pattern matches no_match example ${i + 1}: ${JSON.stringify(ex)}`);
            });
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  },
};
