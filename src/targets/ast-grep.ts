/** `ast-grep`: a structural pattern Forge owns (.forge/ast-grep/rules), optionally with engine capabilities (§15.1). */
import { mkdirSync, mkdtempSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { astGrepRulePath, astGrepTestPath, renderAstGrepRule, renderAstGrepTest, renderSgConfig, typesLanguage } from "../render.js";
import { astGrepCommand, typesExamples } from "../selftest/runners.js";
import type { AstGrepTarget, ToolTarget } from "../types.js";
import { TY_VERSION, run } from "../util.js";
import { patternEvents } from "./events.js";
import type { TargetKind } from "./kind.js";

export const MODES = ["forbid", "no-increase", "no-decrease", "advise"] as const;

export const astGrep: TargetKind<AstGrepTarget> = {
  kind: "ast-grep",
  phase: 1,
  schema: {
    additionalProperties: false,
    required: ["mode", "rule"],
    properties: {
      target: true,
      mode: { enum: [...MODES] },
      rule: { type: "object", minProperties: 1 },
      constraints: { type: "object" },
      utils: { type: "object" },
      where: { type: "object", minProperties: 1, propertyNames: { pattern: "^[A-Z][A-Z0-9_]*$" }, additionalProperties: { $ref: "#/$defs/where" } },
      // packages (type stubs) the types engine needs to see the types the where names
      needs: {
        type: "array",
        minItems: 1,
        items: {
          type: "object",
          additionalProperties: false,
          required: ["ecosystem", "name"],
          properties: { ecosystem: { enum: ["pypi", "npm"] }, name: { type: "string", minLength: 1 } },
        },
      },
    },
    allOf: [
      // engine capabilities decide on single matches: counted modes are not supported yet
      { if: { required: ["where"] }, then: { properties: { mode: { enum: ["forbid", "advise"] } } } },
      { if: { required: ["needs"] }, then: { required: ["where"] } },
    ],
  },
  label: (t) => `ast-grep ${t.mode}`,
  validate(rule, t) {
    // deprecated uses are answered by the TypeScript engine (§15.7)
    if (rule.scope.language === "python" && JSON.stringify(t.where ?? {}).includes('"deprecated"'))
      throw new Error("invalid rule: where deprecated is answered for TypeScript only (§15.7); Python's deprecations are ty's deprecated rule");
  },
  needsExamples: () => true,
  plan(ctx, rule, st, ast) {
    // the stand-in for a tool target whose tool is absent; a switched-off diagnostic is not asked for in any form
    const tools = rule.enforce.filter((t): t is ToolTarget => t.target === "tool");
    if (tools.some((t) => ctx.switchedOff(t)) || (tools.length > 0 && tools.some((t) => ctx.present(t.tool)))) return;
    if (ctx.level === "user") {
      ctx.warnings.push(`${rule.id}: ast-grep targets are project-level only`);
      return;
    }
    const missing = ctx.missingNeeds(rule, ast);
    if (missing.length) {
      st.notEnforced.push(`not enforced here: the types engine needs ${missing.join(", ")} installed to see the types this rule checks (add it as a development dependency)`);
      return;
    }
    const dirs = ctx.ruleDirs(rule);
    st.astgrep = ast;
    ctx.out.astgrep.push({ rule, target: ast, ...(dirs ? { dirs } : {}) });
    const lang = typesLanguage(rule, ast);
    const engine = lang === "python" ? `ty ${TY_VERSION}` : "the project's TypeScript";
    st.enforced.push(`ast-grep ${ast.mode} (.forge/ast-grep/${astGrepRulePath(rule.id)})${lang ? ` with types (${engine})` : ""}${dirs ? ` (in ${dirs.join(", ")})` : ""}`);
    st.notEnforced = [];
  },
  enforcement: (st) => (st.astgrep ? { by: `ast-grep ${st.astgrep.mode}` } : {}),
  lifecycle(rule, t) {
    const tools = rule.enforce.filter((x): x is ToolTarget => x.target === "tool");
    return patternEvents(rule, "ast-grep", t.mode, tools.length ? `without ${tools.map((x) => x.tool).join(" or ")}` : undefined);
  },
  selftest(rules, { report }) {
    const { failures, counts } = report;
    // structural checks the schema cannot express; engine capabilities (§15.1): metavariables of the rule, a parsed language, examples with their packages
    for (const r of rules) {
      const ast = r.enforce.filter((t): t is AstGrepTarget => t.target === "ast-grep");
      if (ast.length > 1) failures.push(`${r.id}: at most one ast-grep target`);
      if (r.scope.language === "any") failures.push(`${r.id}: ast-grep targets need a concrete scope.language`);
      const typed = ast.find((t) => !!t.where);
      if (!typed) continue;
      const text = JSON.stringify([typed.rule, typed.constraints ?? {}, typed.utils ?? {}]);
      for (const v of Object.keys(typed.where!)) if (!text.includes(`$${v}`)) failures.push(`${r.id}: where: $${v} is not a metavariable of the rule`);
      if (!["python", "typescript", "tsx"].includes(r.scope.language)) failures.push(`${r.id}: where needs scope.language python, typescript or tsx`);
      const eco = typesLanguage(r, typed) === "python" ? "pypi" : "npm";
      // without examples.deps, the examples are type-checked against the standard library (Python) or TypeScript's own lib only
      if (r.examples?.deps?.some((d) => d.ecosystem !== eco)) failures.push(`${r.id}: examples.deps must be ${eco} packages`);
      for (const n of typed.needs ?? [])
        if (!r.examples?.deps?.some((d) => d.ecosystem === n.ecosystem && d.name === n.name)) failures.push(`${r.id}: needs ${n.name}, so examples.deps must install it`);
    }

    // the rule-tests: every rule compiled, its examples as valid / invalid cases
    counts.astgrep = rules.length;
    const cmd = astGrepCommand();
    if (!cmd) failures.push("ast-grep: neither bunx nor uvx is available");
    else {
      const dir = mkdtempSync(join(tmpdir(), "forge-test-"));
      try {
        writeFileSync(join(dir, "sgconfig.yml"), renderSgConfig());
        for (const r of rules) {
          const t = r.enforce.find((x): x is AstGrepTarget => x.target === "ast-grep")!;
          for (const [rel, text] of [
            [astGrepRulePath(r.id), renderAstGrepRule(r, t)],
            [astGrepTestPath(r.id), renderAstGrepTest(r)],
          ] as const) {
            mkdirSync(dirname(join(dir, rel)), { recursive: true });
            writeFileSync(join(dir, rel), text);
          }
        }
        const res = run(cmd[0]!, [...cmd.slice(1), "test", "--skip-snapshot-tests", "-c", join(dir, "sgconfig.yml")], { cwd: dir });
        const out = res.stdout + res.stderr;
        const ran = /Running (\d+) tests/.exec(out);
        const files = readdirSync(join(dir, "rules"), { recursive: true }).filter((f) => String(f).endsWith(".yml")).length;
        if (!ran) failures.push(`ast-grep test did not run:\n${out.trim()}`);
        else if (Number(ran[1]) !== files) failures.push(`ast-grep loaded ${ran[1]} rules, expected ${files}`);
        if (res.status !== 0) {
          const failed = out
            .split("\n")
            .filter((l) => /^(FAIL|\[|Error|error)/.test(l.trim()) || /incorrect|Noisy|Missing/i.test(l))
            .slice(0, 40);
          failures.push(`ast-grep test failed:\n${(failed.length ? failed : out.trim().split("\n").slice(-30)).join("\n")}`);
        }
      } finally {
        rmSync(dir, { recursive: true, force: true });
      }
    }

    // examples of rules with type constraints, through the types engine (§15.1)
    const typedRules = rules.filter((r) => r.enforce.some((t) => t.target === "ast-grep" && t.where));
    counts.types = typedRules.length;
    if (typedRules.length) typesExamples(typedRules, report);
  },
};
