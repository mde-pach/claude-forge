import { join } from "node:path";
import { readText } from "../core/fs.js";
import { MANIFEST_PATH } from "../core/paths.js";
import { renderLock, renderManifest } from "../lock.js";
import { roleGlobs } from "../roles.js";
import type { Plan } from "../plan/index.js";
import { blockHash, CHECK, hasBlock, insertBlock } from "../precommit.js";
import { addEntries, conflictingScalars, entryKey, hasEntry, removeEntries } from "../settings.js";
import { lineDiff, ownedHash } from "../util.js";
import type { SettingsEntry } from "../types.js";
import { type Ctx, formatJson } from "./context.js";
import { ForgeError, out } from "./io.js";

export interface Changes {
  writes: { path: string; before?: string; after: string }[];
  deletes: string[];
  drift: string[];
  conflicts: string[];
  settings: { file: string; before: string; after?: string; shared: boolean }[];
  /** Forge's pre-commit block re-added to a hook manager's file (apply adds back a missing block). */
  blocks: { path: string; before?: string; after: string; created: boolean }[];
  /** Hook manager files whose Forge block was edited (left as is). */
  blockDrift: string[];
  lockSettings: SettingsEntry[];
  missingEntries: SettingsEntry[];
  rulesChanged: string[];
  /** File roles whose effective patterns changed since the last apply (a tool config or forge.yaml changed, §15.4). */
  rolesChanged: string[];
  /** Scalar settings the user set differently; never overwritten. */
  overridden: string[];
  lockText: string;
  /** .forge/manifest.sha256 (project level), or undefined. */
  manifestText?: string;
}

export function computeChanges(ctx: Ctx): Changes {
  const { base, plan: p, lock } = ctx;
  const ch: Changes = {
    writes: [],
    deletes: [],
    drift: [],
    conflicts: [],
    settings: [],
    blocks: [],
    blockDrift: [],
    lockSettings: [],
    missingEntries: [],
    rulesChanged: [],
    rolesChanged: [],
    overridden: [],
    lockText: "",
  };
  const owned = lock?.files ?? {};
  for (const [path, after] of p.files) {
    const before = readText(join(base, path));
    // owned hashes ignore JSON whitespace: a file the project's formatter reflowed is neither drift nor rewritten
    const same = before !== undefined && ownedHash(path, before) === ownedHash(path, after);
    if (before !== undefined && path in owned && ownedHash(path, before) !== owned[path]) ch.drift.push(path);
    else if (before !== undefined && !(path in owned) && !same) ch.conflicts.push(path);
    if (!same) ch.writes.push({ path, before, after });
  }
  for (const [path, hash] of Object.entries(owned)) {
    if (p.files.has(path)) continue;
    const before = readText(join(base, path));
    if (before === undefined) continue;
    if (ownedHash(path, before) !== hash) ch.drift.push(path);
    ch.deletes.push(path);
  }

  // settings: re-add recorded entries, add new ones, remove entries no longer wanted
  const desired = new Set(p.settings.map(entryKey));
  const old = lock?.settings ?? [];
  const files = [...new Set([p.settingsFile, ...old.map((e) => e.file)])];
  const kept: SettingsEntry[] = [];
  for (const file of files) {
    try {
      settingsChanges(file);
    } catch (e) {
      throw new ForgeError(`forge: ${file}: ${(e as Error).message}; fix it and re-run`);
    }
  }
  function settingsChanges(file: string): void {
    const before = readText(join(base, file)) ?? "";
    let text: string | undefined = before;
    const drop = old.filter((e) => e.file === file && !desired.has(entryKey(e)));
    if (drop.length && text) text = removeEntries(text, drop);
    const want = p.settings.filter((e) => e.file === file);
    for (const c of conflictingScalars(before, want)) ch.overridden.push(`${file}: ${c.entry.path} is ${JSON.stringify(c.current)}; forge wants ${JSON.stringify(c.entry.value)} (left unchanged)`);
    for (const e of old)
      if (e.file === file && desired.has(entryKey(e))) {
        if (!hasEntry(before, e)) ch.missingEntries.push(e);
        kept.push(e);
      }
    let added: SettingsEntry[] = [];
    if (want.length) {
      const r = addEntries(text ?? "", want);
      text = r.text;
      added = r.added;
    }
    for (const e of added) if (!kept.some((k) => entryKey(k) === entryKey(e))) kept.push(e);
    let after = text === undefined || text === "" ? undefined : text;
    // a settings file Forge creates is formatted like the rest of its JSON; an existing one keeps its own indentation
    if (after !== undefined && before === "" && ctx.level === "project") after = formatJson(base, file, after);
    if ((after ?? "") !== before) ch.settings.push({ file, before, after, shared: !file.endsWith("settings.local.json") });
  }
  const order = new Map(p.settings.map((e, i) => [entryKey(e), i]));
  ch.lockSettings = kept.sort((a, b) => (order.get(entryKey(a)) ?? 0) - (order.get(entryKey(b)) ?? 0));

  for (const b of ctx.blocks) {
    const before = readText(join(base, b.file));
    if (before !== undefined && hasBlock(before)) {
      if (blockHash(before) !== b.hash) ch.blockDrift.push(b.file);
      continue;
    }
    const after = insertBlock(b.kind, before);
    if (after === undefined) ch.overridden.push(`${b.file}: forge's pre-commit block cannot be added back (add "${CHECK}" yourself)`);
    else ch.blocks.push({ path: b.file, before, after, created: before === undefined });
  }

  for (const [id, hash] of Object.entries(lock?.rules ?? {})) if (p.rules[id] !== undefined && p.rules[id] !== hash) ch.rulesChanged.push(id);
  const roles = Object.fromEntries(p.roles.map((r) => [r.name, roleGlobs(r)]));
  for (const r of p.roles) {
    const was = lock?.roles?.[r.name];
    if (was && JSON.stringify(was) !== JSON.stringify(roles[r.name])) ch.rolesChanged.push(`files @${r.name} changed since the last apply: ${roleSummary(r)}`);
  }
  const hashes: Record<string, string> = {};
  for (const [path, text] of p.files) hashes[path] = ownedHash(path, text);
  ch.lockText = renderLock(p.rules, hashes, ch.lockSettings, ctx.blocks, roles);
  if (ctx.level === "project") {
    ch.lockText = formatJson(base, p.lockPath, ch.lockText);
    ch.manifestText = renderManifest(hashes);
  }
  return ch;
}

/** A role's patterns with their sources: "a/**, b/** (alembic.ini); c/** (forge.yaml)". */
export function roleSummary(r: Plan["roles"][number]): string {
  if (!r.patterns.length) return "no patterns";
  const bySource = new Map<string, string[]>();
  for (const p of r.patterns) bySource.set(p.source, [...(bySource.get(p.source) ?? []), `${p.exclude ? "!" : ""}${p.glob}`]);
  return [...bySource.entries()].map(([src, globs]) => `${globs.join(", ")} (${src})`).join("; ");
}

export function printDiff(ctx: Ctx, ch: Changes, opts: { withLock: boolean }): boolean {
  let any = false;
  for (const w of ch.writes) {
    process.stdout.write(lineDiff(w.before ?? "", w.after, w.before === undefined ? `${w.path} (new)` : w.path));
    any = true;
  }
  for (const d of ch.deletes) {
    out(`--- ${d} (deleted)`);
    any = true;
  }
  for (const b of ch.blocks) {
    process.stdout.write(lineDiff(b.before ?? "", b.after, b.before === undefined ? `${b.path} (new)` : b.path));
    any = true;
  }
  for (const s of ch.settings) {
    process.stdout.write(lineDiff(s.before, s.after ?? "", s.after === undefined ? `${s.file} (deleted)` : s.file));
    any = true;
  }
  for (const e of ch.missingEntries) out(`forge: ${e.file}: entry missing, apply re-adds it: ${e.path} ${JSON.stringify(e.value)}`);
  for (const id of ch.rulesChanged) out(`forge: rule ${id} changed since the last apply`);
  for (const r of ch.rolesChanged) out(`forge: ${r}`);
  if (opts.withLock) {
    // the lock compares as JSON: a formatter reflowing it is not a change
    const before = readText(join(ctx.base, ctx.plan.lockPath)) ?? "";
    const parsed = (t: string) => {
      try {
        return JSON.stringify(JSON.parse(t));
      } catch {
        return t;
      }
    };
    if (parsed(before) !== parsed(ch.lockText)) {
      process.stdout.write(lineDiff(before, ch.lockText, ctx.plan.lockPath));
      any = true;
    }
    if (ch.manifestText !== undefined) {
      const m = readText(join(ctx.base, MANIFEST_PATH)) ?? "";
      if (m !== ch.manifestText) {
        process.stdout.write(lineDiff(m, ch.manifestText, MANIFEST_PATH));
        any = true;
      }
    }
  }
  return any || ch.missingEntries.length > 0 || ch.rulesChanged.length > 0 || ch.rolesChanged.length > 0;
}
