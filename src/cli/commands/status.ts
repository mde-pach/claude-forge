import { auditRules, untypedPackages } from "../../audit.js";
import { FORGE_VERSION } from "../../util.js";
import { computeChanges } from "../changes.js";
import { type Ctx, context } from "../context.js";
import type { Flags } from "../flags.js";
import { out } from "../io.js";
import { notChecked, preCommitState } from "../pre-commit.js";
import { defaultBranch, enforcement, enforcementSummary, formatAudit, formatMerged, mergedSnippets, missingTools, roleFiles, sandboxProblem } from "../report.js";

export function cmdStatus(flags: Flags): number {
  const ctx = context(flags);
  const p = ctx.plan;
  const audits = ctx.level === "project" ? auditRules(ctx.base, p.active, (t) => p.toolDirs[t], p.engineSettings) : [];
  const missing = missingTools(ctx);
  const rules = enforcement(ctx, audits, missing);
  const untyped = untypedState(ctx);
  if (flags.json) {
    const ch = computeChanges(ctx);
    out(
      JSON.stringify(
        {
          forge_version: FORGE_VERSION,
          level: ctx.level,
          base: ctx.base,
          applied: ctx.lock?.forge_version ?? null,
          packs: p.packs,
          inactive_packs: p.removedPacks,
          counts: { active: rules.length, enforced: rules.filter((r) => r.enforced).length, not_enforced: rules.filter((r) => !r.enforced).length },
          rules,
          disabled: p.disabled,
          gated: p.gated,
          switched_off: p.switchedOff,
          deprecated: p.deprecated,
          audits,
          snippets: mergedSnippets(audits),
          missing_tools: missing,
          untyped_packages: untyped,
          pre_commit: ctx.level === "project" ? preCommitState(ctx) : undefined,
          drift: [...ch.drift, ...ch.conflicts, ...ch.blockDrift],
          files: roleFiles(ctx),
          warnings: p.warnings,
        },
        null,
        2,
      ),
    );
    return 0;
  }
  out(`forge ${FORGE_VERSION}: ${ctx.level} level, ${ctx.base}`);
  out(ctx.lock ? `applied with forge ${ctx.lock.forge_version}` : "not applied yet (run forge apply)");
  out("");
  out("Packs:");
  for (const pk of p.packs) out(`  ${pk.id} (${pk.why})`);
  for (const pk of p.removedPacks) out(`  ${pk.id}: not active (${pk.why})`);
  for (const w of p.warnings) out(`forge: ${w}`);
  out("");
  out(`Rules: ${enforcementSummary(rules)}`);
  for (const st of p.active) {
    out(`  ${st.rule.id}${st.rule.origin === "local" ? " (local)" : ""}`);
    for (const e of st.enforced) out(`    ${e}`);
    for (const n of st.notEnforced) out(`    ${n}`);
  }
  if (p.disabled.length) {
    out("");
    out("Disabled:");
    for (const d of p.disabled) out(`  ${d.id}: ${d.reason}`);
  }
  if (p.gated.length) {
    out("");
    out("Inactive (version gate):");
    for (const g of p.gated) out(`  ${g.id}: needs ${g.needs}`);
  }
  if (p.switchedOff.length) {
    out("");
    out("Switched off by packs:");
    for (const o of p.switchedOff) out(`  ${o.tool} ${o.expects} (${o.pack}): ${o.reason}${o.rules.length ? `; not asked for by ${o.rules.join(", ")}` : ""}`);
  }
  if (p.deprecated.length) for (const d of p.deprecated) out(`  ${d.id}: deprecated, replaced by ${d.replaced_by}`);
  if (ctx.level === "project" && p.roles.length) {
    out("");
    out("Files:");
    for (const r of roleFiles(ctx)) {
      out(`  @${r.name}: ${r.summary}${r.matching === 0 ? "; matches no file here" : ""}`);
      for (const n of r.notes) out(`    ${n}`);
    }
  }
  if (ctx.level === "project") {
    if (audits.length) {
      out("");
      out("Audit:");
      for (const a of audits) out("  " + formatAudit(a).replace(/\n/g, "\n  "));
      for (const l of formatMerged(audits)) out(`  ${l}`);
    }
  }
  out("");
  const ch = computeChanges(ctx);
  const drift = [...ch.drift.map((d) => `${d} modified since the last apply`), ...ch.conflicts.map((c) => `${c} is not a forge file`)];
  for (const id of ch.rulesChanged) drift.push(`rule ${id} changed since the last apply`);
  drift.push(...ch.rolesChanged);
  for (const e of ch.missingEntries) drift.push(`${e.file}: missing ${e.path} ${JSON.stringify(e.value)}`);
  for (const f of ch.blockDrift) drift.push(`${f}: forge's pre-commit block modified since the last apply`);
  for (const b of ch.blocks) drift.push(`${b.path}: forge's pre-commit block is missing (apply adds it back)`);
  drift.push(...ch.overridden);
  if (ctx.lock && (ch.writes.length || ch.deletes.length)) drift.push(`${ch.writes.length + ch.deletes.length} file(s) differ from what apply would write (forge diff)`);
  out(drift.length ? "Drift:\n" + drift.map((d) => `  ${d}`).join("\n") : "Drift: none");
  const sandbox = sandboxProblem(ctx);
  if (sandbox) out(sandbox.replace(/^forge: sandbox: /, "Sandbox: "));
  out(missing.length ? "Missing tools:\n" + missing.map((m) => `  ${m}`).join("\n") : "Missing tools: none");
  if (untyped) out(formatUntyped(untyped));
  if (ctx.level === "project") {
    out("");
    const pre = preCommitState(ctx);
    out(pre.installed ? `Pre-commit: ${pre.file}${pre.forge ? " (forge)" : ""}` : `Pre-commit: ${notChecked(ctx, pre)}`);
    out(`CI step, pull requests: sh .forge/check.sh --since origin/${defaultBranch(ctx.base)}`);
    out("CI step, the default branch: sh .forge/check.sh --all");
    if (p.packs.some((pk) => pk.id === "docker"))
      out(
        'Docker: Claude Code\'s sandbox blocks docker. To let Claude run it, add "sandbox": {"excludedCommands": ["docker compose *"]} (or "docker *") to your settings; those commands then run outside the sandbox, guarded by safety/docker-* and the docker/compose rules.',
      );
  }
  return 0;
}

/** Installed packages without typing (§15.2): where ty runs (python/typecheck, the types engine), what it checks as Any here. */
export interface UntypedState {
  setting: "any" | "source";
  count: number;
  names: string[];
}

function untypedState(ctx: Ctx): UntypedState | undefined {
  const p = ctx.plan;
  if (ctx.level !== "project" || !(p.types.includes("python") || /\bforge_ty\b/.test(p.checksFragment ?? ""))) return undefined;
  const setting = p.engineSettings.untypedPackages;
  const names = setting === "any" ? untypedPackages(ctx.base, p.engineSettings) : [];
  return { setting, count: names.length, names };
}

/** One line: how many, the first names, and the opt-out. */
export function formatUntyped(u: UntypedState): string {
  if (u.setting === "source") return "Untyped packages: ty infers them from their source (forge.yaml python: {untyped-packages: source})";
  if (!u.count) return "Untyped packages: none installed (every package ty sees ships typing)";
  const shown = u.names.slice(0, 8).join(", ");
  const more = u.count > 8 ? `, … ${u.count - 8} more` : "";
  return `Untyped packages: ${u.count} installed without typing, checked as Any by ty (${shown}${more}); to infer them from their source: forge.yaml python: {untyped-packages: source}`;
}
