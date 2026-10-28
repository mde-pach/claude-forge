/** The commit-time check (§10.3): Forge's own hook, or a marked block in a hook manager's config. */
import { existsSync } from "node:fs";
import { join } from "node:path";
import { readText } from "../core/fs.js";
import { blockHash, CHECK, hasBlock, hookManager, insertBlock, managerInactive, preCommitPath, runsForgeCheck } from "../precommit.js";
import { type Ctx, PRE_COMMIT } from "./context.js";
import { confirm, showDiff } from "./io.js";

/**
 * Offers the commit-time check: Forge's own .git/hooks/pre-commit, or, with a hook
 * manager, a marked block in its config (owned through the lock's blocks).
 */
export function offerPreCommit(ctx: Ctx): boolean {
  if (ctx.level !== "project" || ctx.flags.dryRun) return false;
  const p = preCommitPath(ctx.base);
  if (ctx.plan.files.has(p) || ctx.blocks.length) return false;
  if (!ctx.flags.yes && !ctx.flags.preCommit && !process.stdin.isTTY) return false;
  const m = hookManager(ctx.base);
  if (!m) {
    if (existsSync(join(ctx.base, p))) return false;
    if (!confirm(`forge: create ${p} running "${CHECK}"?`, ctx.flags.yes || ctx.flags.preCommit)) return false;
    ctx.plan.files.set(p, PRE_COMMIT);
    return true;
  }
  if (!m.file) return false;
  // the manager's config is the team's, usually tracked: --yes alone (scripts, CI) does not edit it
  if (!ctx.flags.preCommit && !process.stdin.isTTY) return false;
  const before = readText(join(ctx.base, m.file));
  if (before !== undefined && runsForgeCheck(before)) return false;
  const after = insertBlock(m.kind, before);
  if (after === undefined) return false;
  showDiff(before, after, m.file);
  if (!ctx.flags.preCommit && !confirm(`forge: add "${CHECK}" to ${m.file} (${m.name})?`, false)) return false;
  ctx.blocks.push({ file: m.file, kind: m.kind, created: before === undefined, hash: blockHash(after)! });
  return true;
}

/**
 * Whether commits are checked: Forge's hook or block (forge), or the user's own line,
 * in a file git runs. A hook manager's config only counts when the manager is
 * installed in this clone; otherwise `activate` is the command that installs it.
 */
export interface PreCommitState {
  installed: boolean;
  file?: string;
  forge?: boolean;
  activate?: string;
}

export function preCommitState(ctx: Ctx): PreCommitState {
  const pc = preCommitPath(ctx.base);
  if (ctx.lock?.files[pc] !== undefined && existsSync(join(ctx.base, pc))) return { installed: true, file: pc, forge: true };
  const m = hookManager(ctx.base);
  let found: { file: string; forge: boolean } | undefined;
  for (const b of ctx.lock?.blocks ?? []) if (!found && hasBlock(readText(join(ctx.base, b.file)) ?? "")) found = { file: b.file, forge: true };
  const own = m?.file ?? (m ? undefined : pc);
  if (!found && own && runsForgeCheck(readText(join(ctx.base, own)) ?? "")) found = { file: own, forge: false };
  if (!found) return { installed: false };
  const activate = m && found.file === m.file ? managerInactive(ctx.base, m) : undefined;
  return activate ? { installed: false, ...found, activate } : { installed: true, ...found };
}

/** The line apply and status print when commits are not checked. */
export function notChecked(ctx: Ctx, st: PreCommitState): string {
  if (st.activate)
    return `${st.file} runs the check${st.forge ? " (forge)" : ""}, but ${hookManager(ctx.base)!.name} is not installed in this clone, so commits are not checked. To activate: ${st.activate}`;
  return `NOT INSTALLED, so commits are not checked. To install: ${preCommitHint(ctx.base)}`;
}

/** How to install the commit-time check. */
export function preCommitHint(base: string): string {
  const m = hookManager(base);
  const pc = preCommitPath(base);
  if (!m && existsSync(join(base, pc))) return `add "${CHECK}" to ${pc} (forge does not touch an existing hook)`;
  if (!m) return `run forge apply interactively (or with --yes) to create ${pc}, or add "${CHECK}" to your hook manager`;
  if (!m.file || insertBlock(m.kind, readText(join(base, m.file))) === undefined) return `add "${CHECK}" to ${m.name}`;
  return `run forge apply interactively (or with --pre-commit) to add "${CHECK}" to ${m.file}, or add it yourself`;
}
