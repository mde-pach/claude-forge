/** forge apply: write the setup. */
import { chmodSync } from "node:fs";
import { join } from "node:path";
import { readText } from "../../core/fs.js";
import { MANIFEST_PATH } from "../../core/paths.js";
import { auditRules } from "../../audit.js";
import { computeChanges, printDiff } from "../changes.js";
import { claudeGate, context } from "../context.js";
import type { Flags } from "../flags.js";
import { confirmSettings, deleteFile, err, out, showDiff, writeSettingsFile, writeText } from "../io.js";
import { notChecked, offerPreCommit, preCommitHint, preCommitState } from "../pre-commit.js";
import { formatAudit, formatMerged, missingTools, provisionEngines, sandboxProblem, toolVersionReport } from "../report.js";
import { pythonEnvState } from "../../audit.js";
import { formatPythonEnv } from "./status.js";

export function cmdApply(flags: Flags): number {
  claudeGate();
  const ctx = context(flags);
  for (const w of ctx.plan.warnings) err(`forge: ${w}`);
  let ch = computeChanges(ctx);
  if ((ch.drift.length || ch.conflicts.length) && !flags.force) {
    for (const path of ch.drift) {
      err(`forge: ${path} was modified since the last apply`);
      showDiff(readText(join(ctx.base, path)), ctx.plan.files.get(path), path, path);
    }
    for (const path of ch.conflicts) {
      err(`forge: ${path} exists and is not a forge file`);
      showDiff(readText(join(ctx.base, path)), ctx.plan.files.get(path), path, path);
    }
    err("forge: stopped. Re-run with --force to overwrite, or move the files away.");
    return 1;
  }
  if (flags.dryRun) {
    printDiff(ctx, ch, { withLock: true });
    return 0;
  }
  confirmSettings(
    ch.settings.filter((x) => x.shared),
    flags.yes,
    "forge: cancelled; nothing written",
  );
  if (offerPreCommit(ctx)) ch = computeChanges(ctx);

  for (const w of ch.writes) writeText(join(ctx.base, w.path), w.after);
  for (const b of ch.blocks) {
    writeText(join(ctx.base, b.path), b.after);
    if (b.created && !/\.ya?ml$/.test(b.path)) chmodSync(join(ctx.base, b.path), 0o755);
  }
  for (const d of ch.deletes) deleteFile(ctx.base, d);
  for (const s of ch.settings) writeSettingsFile(ctx.base, s.file, s.after);
  writeText(join(ctx.base, ctx.plan.lockPath), ch.lockText);
  if (ch.manifestText !== undefined) writeText(join(ctx.base, MANIFEST_PATH), ch.manifestText);

  for (const o of ch.overridden) err(`forge: ${o}`);
  const changed = ch.writes.length + ch.deletes.length + ch.settings.length + ch.blocks.length;
  out(changed ? `forge: applied (${ch.writes.length + ch.blocks.length} written, ${ch.deletes.length} deleted, ${ch.settings.length} settings file(s) updated)` : "forge: up to date");
  if (ctx.level === "project") {
    const audits = auditRules(ctx.base, ctx.plan.active, (t) => ctx.plan.toolDirs[t], ctx.plan.engineSettings);
    for (const a of audits.filter((x) => x.status === "unsatisfied")) out(formatAudit(a));
    for (const l of formatMerged(audits)) out(l);
    const pre = preCommitState(ctx);
    if (pre.activate) out(`forge: pre-commit: ${notChecked(ctx, pre)}`);
    else if (!pre.installed) out(`forge: pre-commit: not installed, so commits are not checked; ${preCommitHint(ctx.base)}`);
    for (const problem of provisionEngines(ctx.plan.types)) out(problem);
    // what the installed tools and environment leave unenforced: for the person configuring Forge,
    // here and in forge status, never in the hooks' output to Claude (§8.1)
    const missing = missingTools(ctx);
    if (missing.length) out(`forge: missing tools (checks using them are skipped): ${missing.join(", ")}`);
    const versions = toolVersionReport(ctx);
    if (versions.length) out(`forge: tool versions (what they cannot do is not enforced):\n${versions.map((v) => `  ${v}`).join("\n")}`);
    if (ctx.plan.packs.some((pk) => pk.id === "python")) {
      const env = pythonEnvState(ctx.base);
      if (!env.chosen || (env.chosen.declared > 0 && env.chosen.installed === 0)) out(`forge: ${formatPythonEnv(env)}`);
    }
  }
  const sandbox = sandboxProblem(ctx);
  if (sandbox) out(sandbox);
  return 0;
}
