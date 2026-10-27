import { chmodSync, cpSync, existsSync, mkdirSync, readdirSync, readFileSync, rmdirSync, rmSync, statSync, writeFileSync, readSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join } from "node:path";
import { checkTools, renderLib } from "./render.js";
import { auditRules, projectBinary, type AuditResult } from "./audit.js";
import { MANIFEST_PATH, readLock, renderLock, renderManifest } from "./lock.js";
import { globToRegExp, loadLocalRules, loadPackBundle } from "./packs.js";
import { roleGlobs } from "./roles.js";
import { plan, readProjectInputs, type Plan } from "./plan.js";
import { blockHash, CHECK, hasBlock, hookManager, insertBlock, managerInactive, onlyForge, preCommitPath, removeBlock, runsForgeCheck } from "./precommit.js";
import { localSelfTest, testSummary } from "./selftest.js";
import { mergeTomlSnippets } from "./snippets.js";
import { addEntries, conflictingScalars, entryKey, hasEntry, removeEntries } from "./settings.js";
import { CLAUDE_CODE_MIN, ENGINE_PYTHON, FORGE_VERSION, TY_VERSION, compareVersions, gitToplevel, lineDiff, listRepoFiles, ownedHash, packageRoot, run, runScript } from "./util.js";
import type { Level, Lock, LockBlock, SettingsEntry } from "./types.js";
import { USAGE } from "./usage.js";
import { applyEdits, modify } from "jsonc-parser/lib/esm/main.js";

class ForgeError extends Error {
  constructor(
    message: string,
    readonly code = 1,
  ) {
    super(message);
  }
}

interface Flags {
  user: boolean;
  local: boolean;
  force: boolean;
  dryRun: boolean;
  yes: boolean;
  json: boolean;
  /** Consent to add the commit-time check to a hook manager's (tracked) config. */
  preCommit: boolean;
}



const out = (s = "") => process.stdout.write(s + "\n");
const err = (s: string) => process.stderr.write(s + "\n");

// ---------------------------------------------------------------- helpers

function readText(path: string): string | undefined {
  return existsSync(path) ? readFileSync(path, "utf8") : undefined;
}

function writeText(path: string, text: string): void {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, text);
  if (path.endsWith(".sh") || /\/hooks\/pre-commit$/.test(path)) chmodSync(path, 0o755);
}

/** Removes empty directories from dir up to (not including) stop. */
function pruneDirs(dir: string, stop: string): void {
  while (dir.startsWith(stop + "/") && existsSync(dir) && readdirSync(dir).length === 0) {
    rmdirSync(dir);
    dir = dirname(dir);
  }
}

function confirm(question: string, flags: Flags): boolean {
  if (flags.yes) return true;
  if (!process.stdin.isTTY) throw new ForgeError(`forge: ${question} Re-run with --yes to confirm non-interactively.`);
  process.stdout.write(`${question} [y/N] `);
  const buf = Buffer.alloc(1);
  let line = "";
  for (;;) {
    let n = 0;
    try {
      n = readSync(0, buf, 0, 1, null);
    } catch {
      break;
    }
    if (n === 0 || buf[0] === 10) break;
    line += String.fromCharCode(buf[0]!);
  }
  return /^y(es)?$/i.test(line.trim());
}

function claudeGate(): void {
  const r = run("claude", ["--version"]);
  if (r.status !== 0) throw new ForgeError(`forge: Claude Code not found; forge needs claude >= ${CLAUDE_CODE_MIN}`);
  const v = /(\d+\.\d+\.\d+)/.exec(r.stdout)?.[1];
  if (!v || compareVersions(v, CLAUDE_CODE_MIN) < 0)
    throw new ForgeError(`forge: Claude Code ${v ?? "(unknown version)"} is too old; forge needs claude >= ${CLAUDE_CODE_MIN}`);
}

// ---------------------------------------------------------------- context

interface Ctx {
  level: Level;
  base: string;
  plan: Plan;
  lock?: Lock;
  flags: Flags;
  /** Forge's blocks in hook managers' files: those of the lock, plus one apply adds. */
  blocks: LockBlock[];
}

function lockOf(base: string, lockPath: string, level: Level): Lock | undefined {
  try {
    return readLock(base, lockPath, level);
  } catch (e) {
    throw new ForgeError(`forge: ${(e as Error).message}`);
  }
}

function packsDir(): string {
  return join(packageRoot(), "packs");
}

/** The packs shipped with Forge, as the build bundled them (dist/packs.json, packBundle). */
function shippedPacks() {
  return loadPackBundle(join(packageRoot(), "dist", "packs.json"), packsDir());
}

function context(flags: Flags): Ctx {
  const level: Level = flags.user ? "user" : "project";
  let base: string;
  if (level === "user") base = homedir();
  else {
    const top = gitToplevel(process.cwd());
    if (!top) throw new ForgeError("forge: not inside a git repository (use --user for the user-level setup)");
    base = top;
  }
  const packs = shippedPacks();
  const p = plan({ level, base, packs, local: flags.local, ...(level === "project" ? readProjectInputs(base) : {}) });
  const lock = lockOf(base, p.lockPath, level);
  const ctx: Ctx = { level, base, plan: p, lock, flags, blocks: level === "project" ? [...(lock?.blocks ?? [])] : [] };
  addPreCommit(ctx);
  if (level === "project") for (const [path, text] of p.files) if (/\.(json|md|yml|mjs)$/.test(path)) p.files.set(path, formatJson(base, path, text));
  return ctx;
}

/**
 * JSON (and Markdown, YAML and JavaScript helpers) Forge generates, formatted with the
 * project's own formatter, so `biome ci .` or `prettier --check .` passes on a fresh apply:
 * JSON and JavaScript helpers with Biome when the project has it, else all of them with
 * Prettier (a helper's integrity hash leaves formatting out, §12). Unchanged when there is
 * neither or it declines (e.g. the path is ignored in its config).
 */
function formatJson(base: string, path: string, text: string): string {
  const biome = /\.(json|mjs)$/.test(path) ? projectBinary(base, "biome") : undefined;
  const prettier = biome ? undefined : projectBinary(base, "prettier");
  if (!biome && !prettier) return text;
  const r = biome ? run(biome, ["format", `--stdin-file-path=${path}`], { cwd: base, input: text }) : run(prettier!, [`--stdin-filepath=${path}`], { cwd: base, input: text });
  return r.status === 0 && r.stdout.trim() ? r.stdout : text;
}

// ---------------------------------------------------------------- pre-commit (§10.3)

const PRE_COMMIT = "#!/bin/sh\n# Generated by forge. Remove with forge remove.\nexec sh .forge/check.sh --changed\n";

/** Keeps a pre-commit hook forge created earlier in the plan. */
function addPreCommit(ctx: Ctx): void {
  if (ctx.level !== "project") return;
  const p = preCommitPath(ctx.base);
  if (ctx.lock?.files[p] !== undefined) ctx.plan.files.set(p, PRE_COMMIT);
}

/**
 * Offers the commit-time check: Forge's own .git/hooks/pre-commit, or, with a hook
 * manager, a marked block in its config (owned through the lock's blocks).
 */
function offerPreCommit(ctx: Ctx): boolean {
  if (ctx.level !== "project" || ctx.flags.dryRun) return false;
  const p = preCommitPath(ctx.base);
  if (ctx.plan.files.has(p) || ctx.blocks.length) return false;
  if (!ctx.flags.yes && !ctx.flags.preCommit && !process.stdin.isTTY) return false;
  const m = hookManager(ctx.base);
  if (!m) {
    if (existsSync(join(ctx.base, p))) return false;
    if (!confirm(`forge: create ${p} running "${CHECK}"?`, { ...ctx.flags, yes: ctx.flags.yes || ctx.flags.preCommit })) return false;
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
  process.stdout.write(lineDiff(before ?? "", after, before === undefined ? `${m.file} (new)` : m.file));
  if (!ctx.flags.preCommit && !confirm(`forge: add "${CHECK}" to ${m.file} (${m.name})?`, { ...ctx.flags, yes: false })) return false;
  ctx.blocks.push({ file: m.file, kind: m.kind, created: before === undefined, hash: blockHash(after)! });
  return true;
}

/**
 * Whether commits are checked: Forge's hook or block (forge), or the user's own line,
 * in a file git runs. A hook manager's config only counts when the manager is
 * installed in this clone; otherwise `activate` is the command that installs it.
 */
interface PreCommitState {
  installed: boolean;
  file?: string;
  forge?: boolean;
  activate?: string;
}
function preCommitState(ctx: Ctx): PreCommitState {
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
function notChecked(ctx: Ctx, st: PreCommitState): string {
  if (st.activate) return `${st.file} runs the check${st.forge ? " (forge)" : ""}, but ${hookManager(ctx.base)!.name} is not installed in this clone, so commits are not checked. To activate: ${st.activate}`;
  return `NOT INSTALLED, so commits are not checked. To install: ${preCommitHint(ctx.base)}`;
}

// ---------------------------------------------------------------- changes

interface Changes {
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

function computeChanges(ctx: Ctx): Changes {
  const { base, plan: p, lock } = ctx;
  const ch: Changes = { writes: [], deletes: [], drift: [], conflicts: [], settings: [], blocks: [], blockDrift: [], lockSettings: [], missingEntries: [], rulesChanged: [], rolesChanged: [], overridden: [], lockText: "" };
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
    for (const c of conflictingScalars(before, want))
      ch.overridden.push(`${file}: ${c.entry.path} is ${JSON.stringify(c.current)}; forge wants ${JSON.stringify(c.entry.value)} (left unchanged)`);
    for (const e of old) if (e.file === file && desired.has(entryKey(e))) {
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
function roleSummary(r: Plan["roles"][number]): string {
  if (!r.patterns.length) return "no patterns";
  const bySource = new Map<string, string[]>();
  for (const p of r.patterns) bySource.set(p.source, [...(bySource.get(p.source) ?? []), `${p.exclude ? "!" : ""}${p.glob}`]);
  return [...bySource.entries()].map(([src, globs]) => `${globs.join(", ")} (${src})`).join("; ");
}

function printDiff(ctx: Ctx, ch: Changes, opts: { withLock: boolean }): boolean {
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

// ---------------------------------------------------------------- commands

function cmdApply(flags: Flags): number {
  claudeGate();
  const ctx = context(flags);
  for (const w of ctx.plan.warnings) err(`forge: ${w}`);
  let ch = computeChanges(ctx);
  if ((ch.drift.length || ch.conflicts.length) && !flags.force) {
    for (const path of ch.drift) {
      err(`forge: ${path} was modified since the last apply`);
      process.stdout.write(lineDiff(readText(join(ctx.base, path)) ?? "", ctx.plan.files.get(path) ?? "", path));
    }
    for (const path of ch.conflicts) {
      err(`forge: ${path} exists and is not a forge file`);
      process.stdout.write(lineDiff(readText(join(ctx.base, path)) ?? "", ctx.plan.files.get(path) ?? "", path));
    }
    err("forge: stopped. Re-run with --force to overwrite, or move the files away.");
    return 1;
  }
  if (flags.dryRun) {
    printDiff(ctx, ch, { withLock: true });
    return 0;
  }
  for (const s of ch.settings.filter((x) => x.shared)) {
    process.stdout.write(lineDiff(s.before, s.after ?? "", s.file));
    if (!confirm(`forge: apply these changes to ${s.file}?`, flags)) throw new ForgeError("forge: cancelled; nothing written");
  }
  if (offerPreCommit(ctx)) ch = computeChanges(ctx);

  for (const w of ch.writes) writeText(join(ctx.base, w.path), w.after);
  for (const b of ch.blocks) {
    writeText(join(ctx.base, b.path), b.after);
    if (b.created && !/\.ya?ml$/.test(b.path)) chmodSync(join(ctx.base, b.path), 0o755);
  }
  for (const d of ch.deletes) {
    rmSync(join(ctx.base, d), { force: true });
    pruneDirs(dirname(join(ctx.base, d)), ctx.base);
  }
  for (const s of ch.settings) {
    if (s.after === undefined) rmSync(join(ctx.base, s.file), { force: true });
    else writeText(join(ctx.base, s.file), s.after);
  }
  writeText(join(ctx.base, ctx.plan.lockPath), ch.lockText);
  if (ch.manifestText !== undefined) writeText(join(ctx.base, MANIFEST_PATH), ch.manifestText);

  for (const o of ch.overridden) err(`forge: ${o}`);
  const changed = ch.writes.length + ch.deletes.length + ch.settings.length + ch.blocks.length;
  out(changed ? `forge: applied (${ch.writes.length + ch.blocks.length} written, ${ch.deletes.length} deleted, ${ch.settings.length} settings file(s) updated)` : "forge: up to date");
  if (ctx.level === "project") {
    const audits = auditRules(ctx.base, ctx.plan.active, (t) => ctx.plan.toolDirs[t]);
    for (const a of audits.filter((x) => x.status === "unsatisfied")) out(formatAudit(a));
    for (const l of formatMerged(audits)) out(l);
    const pre = preCommitState(ctx);
    if (pre.activate) out(`forge: pre-commit: ${notChecked(ctx, pre)}`);
    else if (!pre.installed) out(`forge: pre-commit: not installed, so commits are not checked; ${preCommitHint(ctx.base)}`);
    for (const problem of provisionEngines(ctx.plan.types)) out(problem);
  }
  const sandbox = sandboxProblem(ctx);
  if (sandbox) out(sandbox);
  return 0;
}

/**
 * Fetches the pinned Python types engine into the uvx cache (§15.2), so the hooks start
 * it offline; TypeScript's is the project's own (node_modules). Returns what could not be
 * provisioned.
 */
function provisionEngines(langs: string[]): string[] {
  if (!langs.includes("python")) return [];
  const r = run("uvx", ["-q", "--python", ENGINE_PYTHON, "--from", `ty==${TY_VERSION}`, "ty", "--version"]);
  if (r.status === 0) return [];
  return [`forge: the python types engine (ty ${TY_VERSION}) could not be provisioned with uvx: ${(r.stderr || r.stdout).trim().split("\n").slice(-1)[0] || `exit ${r.status}`}; rules with type constraints are reported unchecked until it is`];
}

/**
 * When the setup enables Claude Code's sandbox on Linux, whether bubblewrap can
 * start it here: a container with Docker's default seccomp/AppArmor profile cannot
 * create namespaces ("Operation not permitted"), and one with a masked /proc cannot
 * mount a new proc, which Claude Code's sandbox does (so the probe passes --proc).
 * Returns the message to print, or undefined when the sandbox can run (or is not
 * Linux's to check).
 */
function sandboxProblem(ctx: Ctx): string | undefined {
  if (process.platform !== "linux") return undefined;
  if (!ctx.plan.settings.some((e) => e.path === "sandbox.enabled" && e.value === true)) return undefined;
  const off = 'or turn the sandbox off knowingly: set "sandbox": {"enabled": false} in ~/.claude/settings.json (forge keeps a value you set)';
  const has = (c: string) => run("sh", ["-c", `command -v ${c}`]).status === 0;
  const tools = ["bwrap", "socat"].filter((c) => !has(c));
  if (tools.length)
    return `forge: sandbox: Claude Code's sandbox needs ${tools.join(" and ")} on Linux (apt install bubblewrap socat, or your distribution's packages), ${off}.`;
  const r = run("bwrap", ["--ro-bind", "/", "/", "--dev", "/dev", "--proc", "/proc", "--unshare-all", "--die-with-parent", "true"]);
  if (r.status === 0) return undefined;
  const why = (r.stderr || r.stdout).trim().split("\n")[0] || `exit ${r.status}`;
  return `forge: sandbox: Claude Code's sandbox cannot start here: bubblewrap failed (${why}). In a container with Docker's default seccomp/AppArmor profile, namespaces are not allowed and /proc is masked. Run the container with what bubblewrap needs (e.g. --security-opt seccomp=unconfined --security-opt apparmor=unconfined --security-opt systempaths=unconfined, or --cap-add SYS_ADMIN), ${off}.`;
}

function cmdDiff(flags: Flags): number {
  const ctx = context(flags);
  const ch = computeChanges(ctx);
  for (const path of ch.drift) out(`forge: ${path} was modified since the last apply`);
  for (const path of ch.conflicts) out(`forge: ${path} exists and is not a forge file`);
  return printDiff(ctx, ch, { withLock: true }) || ch.drift.length > 0 ? 1 : 0;
}

/**
 * One config block per TOML tool (ruff, ty) and audited place for its unsatisfied audits,
 * when there are several: rules add entries to the same tables (extend-select,
 * banned-api), and a table written twice is not valid TOML. Keyed "<tool>" or "<tool> (in <dir>)".
 */
function mergedSnippets(audits: AuditResult[]): Record<string, string> {
  const groups = new Map<string, string[]>();
  for (const a of audits)
    if (a.status === "unsatisfied" && a.snippet && (a.tool === "ruff" || a.tool === "ty")) {
      const k = `${a.tool}${a.dir ? ` (in ${a.dir})` : ""}`;
      if (!groups.has(k)) groups.set(k, []);
      if (!groups.get(k)!.includes(a.snippet)) groups.get(k)!.push(a.snippet);
    }
  const out: Record<string, string> = {};
  for (const [k, list] of groups) {
    const merged = list.length > 1 ? mergeTomlSnippets(list) : undefined;
    if (merged) out[k] = merged;
  }
  return out;
}

function formatMerged(audits: AuditResult[]): string[] {
  return Object.entries(mergedSnippets(audits)).flatMap(([k, block]) => [`forge: to satisfy every unsatisfied ${k} audit, add to the ${k.split(" ")[0]} config:`, ...block.split("\n").map((l) => `  ${l}`)]);
}

function formatAudit(a: AuditResult): string {
  const head = `forge:${a.rule} ${a.tool} ${a.expects}${a.dir ? ` (in ${a.dir})` : ""}: ${a.status}`;
  if (a.status === "satisfied") return head;
  const lines = [head + (a.detail ? ` (${a.detail})` : "")];
  if (a.snippet) lines.push(`  add to the ${a.tool} config: ${a.snippet}`);
  return lines.join("\n");
}

/** How to install the commit-time check. */
function preCommitHint(base: string): string {
  const m = hookManager(base);
  const pc = preCommitPath(base);
  if (!m && existsSync(join(base, pc))) return `add "${CHECK}" to ${pc} (forge does not touch an existing hook)`;
  if (!m) return `run forge apply interactively (or with --yes) to create ${pc}, or add "${CHECK}" to your hook manager`;
  if (!m.file || insertBlock(m.kind, readText(join(base, m.file))) === undefined) return `add "${CHECK}" to ${m.name}`;
  return `run forge apply interactively (or with --pre-commit) to add "${CHECK}" to ${m.file}, or add it yourself`;
}

/**
 * Tools the setup needs that are missing. Project level: the generated scripts'
 * own forge_missing_tools (what SessionStart reports), run here on the checks
 * fragment, so both always agree.
 */
function missingTools(ctx: Ctx): string[] {
  if (ctx.level === "user" || !ctx.plan.checksFragment) {
    const has = (c: string) => run("sh", ["-c", `command -v ${c}`]).status === 0;
    return !has("bunx") && !has("uvx") ? ["bunx or uvx (ast-grep)"] : [];
  }
  const script = `${renderLib()}\nIFS='\n'\n${ctx.plan.checksFragment}\nforge_missing_tools\n`;
  const r = runScript(script, { cwd: ctx.base });
  return r.stdout.trim() ? r.stdout.trim().split(", ") : [];
}

/** The remote's default branch (origin/HEAD), else "main". */
function defaultBranch(base: string): string {
  const r = run("git", ["symbolic-ref", "--quiet", "--short", "refs/remotes/origin/HEAD"], { cwd: base });
  return r.status === 0 && r.stdout.trim().startsWith("origin/") ? r.stdout.trim().slice("origin/".length) : "main";
}

interface Enforcement {
  id: string;
  enforced: boolean;
  /** What enforces it here. */
  by: string[];
  /** Why a target does not enforce it here; the first one is the rule's summary reason. */
  not_enforced: string[];
}

const UNSATISFIED: Record<string, string> = {
  ruff: "ruff codes not selected",
  biome: "Biome rules not at error",
  ty: "ty rules not at error",
  tsc: "tsc options not set",
  eslint: "ESLint rules not at error",
};

/** Whether each active rule is actually enforced in this project, and why not. */
function enforcement(ctx: Ctx, audits: AuditResult[], missing: string[]): Enforcement[] {
  // "tsc (packages/app)": missing in a workspace package
  const absent = new Set(missing.map((m) => m.replace(/ \(.*\)$/, "")));
  return ctx.plan.active.map((st) => {
    const by: string[] = [];
    const not: string[] = [];
    // audited in several packages: enforced by the tool only where every package's config enforces it
    const mine = audits.filter((x) => x.rule === st.rule.id);
    for (const tool of new Set(mine.map((a) => a.tool))) {
      const each = mine.filter((a) => a.tool === tool);
      const bad = each.find((a) => a.status !== "satisfied");
      if (!bad) by.push(`${tool} ${each[0]!.expects}`);
      else if (bad.status === "unsatisfied") not.push(UNSATISFIED[tool]!);
      else not.push(/not installed/.test(bad.detail ?? "") ? `${tool} not installed` : `${tool} audit skipped`);
    }
    for (const n of st.notEnforced) not.push(n.replace(/^not enforced here: /, ""));
    if (st.astgrep) by.push(`ast-grep ${st.astgrep.mode}`);
    for (const t of st.rule.enforce) {
      if (t.target === "permission" || t.target === "guard" || t.target === "sandbox" || t.target === "grep")
        by.push(t.target === "permission" || t.target === "guard" ? `${t.target} ${t.decision}` : t.target);
      else if (t.target === "check" && st.enforced.some((e) => e.startsWith("check "))) {
        const gone = checkTools(t).filter((x) => absent.has(x));
        if (gone.length) not.push(`${gone.join(", ")} not installed`);
        else by.push(`check ${t.stage}`);
      }
    }
    return { id: st.rule.id, enforced: by.length > 0, by: [...new Set(by)], not_enforced: [...new Set(not)] };
  });
}

function enforcementSummary(rules: Enforcement[]): string {
  const off = rules.filter((r) => !r.enforced);
  const reasons = new Map<string, number>();
  for (const r of off) {
    const why = r.not_enforced[0] ?? "nothing applies here";
    reasons.set(why, (reasons.get(why) ?? 0) + 1);
  }
  const detail = [...reasons.entries()].sort((a, b) => b[1] - a[1]).map(([w, n]) => `${n} ${w}`).join(", ");
  return `${rules.length} active, ${rules.length - off.length} enforced, ${off.length} not enforced${off.length ? ` (${detail})` : ""}`;
}

/**
 * The file roles the active rules name (§15.4): their patterns and where each comes from,
 * and how many of the repository's files they match (a role matching nothing is reported,
 * never guessed).
 */
function roleFiles(ctx: Ctx): { name: string; summary: string; patterns: string[]; sources: string[]; matching: number; notes: string[]; rules: string[] }[] {
  if (ctx.level !== "project" || !ctx.plan.roles.length) return [];
  const files = listRepoFiles(ctx.base);
  return ctx.plan.roles.map((r) => {
    const inc = r.patterns.filter((p) => !p.exclude).map((p) => globToRegExp(p.glob));
    const exc = r.patterns.filter((p) => p.exclude).map((p) => globToRegExp(p.glob));
    const matching = files.filter((f) => inc.some((re) => re.test(f)) && !exc.some((re) => re.test(f))).length;
    return { name: r.name, summary: roleSummary(r), patterns: roleGlobs(r), sources: [...new Set(r.patterns.map((p) => p.source))], matching, notes: r.notes, rules: r.rules };
  });
}

function cmdStatus(flags: Flags): number {
  const ctx = context(flags);
  const p = ctx.plan;
  const audits = ctx.level === "project" ? auditRules(ctx.base, p.active, (t) => p.toolDirs[t]) : [];
  const missing = missingTools(ctx);
  const rules = enforcement(ctx, audits, missing);
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

function cmdExplain(id: string | undefined, flags: Flags): number {
  if (!id) throw new ForgeError("usage: forge explain <rule-id>", 2);
  const packs = shippedPacks();
  const top = gitToplevel(process.cwd());
  let rule = [...packs.values()].flatMap((p) => p.rules).find((r) => r.id === id);
  if (top) rule = loadLocalRules(top).find((r) => r.id === id) ?? rule;
  if (!rule) throw new ForgeError(`forge: unknown rule ${id}`);
  out(`# ${rule.file}`);
  process.stdout.write(readFileSync(rule.file, "utf8"));
  out("");
  if (!top && !flags.user) {
    out("Not in a git repository: no project-specific enforcement to show.");
    return 0;
  }
  const ctx = context(flags);
  const st = ctx.plan.active.find((s) => s.rule.id === id);
  const dis = ctx.plan.disabled.find((d) => d.id === id);
  const gate = ctx.plan.gated.find((g) => g.id === id);
  if (dis) out(`Disabled here: ${dis.reason}`);
  else if (gate) out(`Not active here: the rule needs ${gate.needs}.`);
  else if (!st) out(`Not active here: pack ${rule.pack} is not active at ${ctx.level} level.`);
  else {
    out("Enforced here:");
    for (const e of st.enforced) out(`  ${e}`);
    for (const n of st.notEnforced) out(`  ${n}`);
    if (!st.enforced.length) out("  nowhere");
    if (ctx.level === "project") for (const a of auditRules(ctx.base, [st], (t) => ctx.plan.toolDirs[t])) out(formatAudit(a));
  }
  return 0;
}

/** forge test: the project's local rules (.forge/rules); the bundled packs are tested by `bun run test:packs`. */
function cmdTest(): number {
  const top = gitToplevel(process.cwd());
  if (!top) throw new ForgeError("forge: not inside a git repository (forge test checks the rules in .forge/rules)");
  const r = localSelfTest(top);
  if (!r.counts.rules && !r.failures.length) {
    out("forge: no local rules in .forge/rules; nothing to test");
    return 0;
  }
  for (const n of r.notes) out(`forge: ${n}`);
  for (const f of r.failures) err(`forge:test ${f}`);
  out(`forge: ${testSummary(r.counts, `${r.counts.rules} local rule(s)`)}: ${r.failures.length ? `${r.failures.length} failure(s)` : "all passed"}`);
  return r.failures.length ? 1 : 0;
}

function cmdRemove(flags: Flags): number {
  const level: Level = flags.user ? "user" : "project";
  const base = level === "user" ? homedir() : gitToplevel(process.cwd());
  if (!base) throw new ForgeError("forge: not inside a git repository");
  const lockPath = level === "user" ? ".claude/forge.lock.json" : ".forge/forge.lock.json";
  const lock = lockOf(base, lockPath, level);
  if (!lock) throw new ForgeError(`forge: no ${lockPath}; nothing to remove`);
  const settings = new Map<string, { before: string; after?: string }>();
  for (const file of [...new Set(lock.settings.map((e) => e.file))]) {
    const before = readText(join(base, file));
    if (before === undefined) continue;
    settings.set(file, { before, after: removeEntries(before, lock.settings.filter((e) => e.file === file)) });
  }
  const files = Object.keys(lock.files).filter((f) => existsSync(join(base, f)));
  // Forge's blocks in hook managers' files: removed; a file Forge created goes when nothing else is left
  const blocks: { block: LockBlock; before: string; after?: string }[] = [];
  const stuck: string[] = [];
  for (const b of lock.blocks) {
    const before = readText(join(base, b.file));
    if (before === undefined || !hasBlock(before)) continue;
    const after = removeBlock(b.kind, before);
    if (after === undefined) stuck.push(b.file);
    else blocks.push({ block: b, before, after: b.created && onlyForge(after) ? undefined : after });
  }
  // a block whose markers were edited, or whose removal would change the rest of the config, is left to the user
  for (const f of stuck) err(`forge: ${f}: forge's pre-commit block cannot be removed safely (its begin/end markers were changed, or entries were added inside it); remove the lines between "# forge: begin" and "# forge: end" yourself`);
  if (flags.dryRun) {
    for (const f of [...files, lockPath, ...(level === "project" && existsSync(join(base, MANIFEST_PATH)) ? [MANIFEST_PATH] : [])]) out(`--- ${f} (deleted)`);
    for (const [file, s] of settings) process.stdout.write(lineDiff(s.before, s.after ?? "", s.after === undefined ? `${file} (deleted)` : file));
    for (const b of blocks) process.stdout.write(lineDiff(b.before, b.after ?? "", b.after === undefined ? `${b.block.file} (deleted)` : b.block.file));
    return 0;
  }
  for (const [file, s] of settings) {
    if (file.endsWith("settings.local.json") || s.before === (s.after ?? "")) continue;
    process.stdout.write(lineDiff(s.before, s.after ?? "", file));
    if (!confirm(`forge: apply these changes to ${file}?`, flags)) throw new ForgeError("forge: cancelled; nothing removed");
  }
  for (const f of files) {
    const text = readFileSync(join(base, f), "utf8");
    if (ownedHash(f, text) !== lock.files[f]) err(`forge: ${f} was modified since the last apply; removing it anyway`);
    rmSync(join(base, f), { force: true });
    pruneDirs(dirname(join(base, f)), base);
  }
  for (const [file, s] of settings) {
    if (s.after === undefined) rmSync(join(base, file), { force: true });
    else writeText(join(base, file), s.after);
    pruneDirs(dirname(join(base, file)), base);
  }
  for (const { block, before, after } of blocks) {
    if (blockHash(before) !== block.hash) err(`forge: ${block.file}: forge's pre-commit block was modified since the last apply; removing it anyway`);
    if (after === undefined) rmSync(join(base, block.file), { force: true });
    else writeFileSync(join(base, block.file), after);
  }
  rmSync(join(base, lockPath), { force: true });
  if (level === "project") rmSync(join(base, MANIFEST_PATH), { force: true });
  if (level === "project") {
    rmSync(join(base, ".forge", "state"), { recursive: true, force: true });
    pruneDirs(join(base, ".forge"), base);
  }
  pruneDirs(dirname(join(base, lockPath)), base);
  out(
    `forge: removed ${files.length} file(s) and ${lock.settings.length} settings entr${lock.settings.length === 1 ? "y" : "ies"}${blocks.length ? `, and the pre-commit block from ${blocks.map((b) => b.block.file).join(", ")}` : ""}`,
  );
  return stuck.length ? 1 : 0;
}

function cmdInit(flags: Flags): number {
  if (flags.user) throw new ForgeError("forge: init is project-level only", 2);
  claudeGate();
  const top = gitToplevel(process.cwd());
  if (!top) throw new ForgeError("forge: not inside a git repository (run git init first)");
  if (existsSync(join(top, ".forge", "forge.lock.json"))) throw new ForgeError("forge: init is for new projects; this one already has forge (use forge apply)");
  const packs = shippedPacks();
  const p = plan({ level: "project", base: top, packs, ...readProjectInputs(top) });
  for (const pk of p.packs) {
    const dir = packs.get(pk.id)!.initDir;
    if (!dir) continue;
    for (const f of readdirSync(dir, { recursive: true }).map(String).sort()) {
      const src = join(dir, f);
      if (statSync(src).isDirectory()) continue;
      const dest = join(top, f);
      if (existsSync(dest)) out(`forge: ${f} exists, kept`);
      else if (flags.dryRun) out(`forge: would create ${f} (from ${pk.id})`);
      else {
        mkdirSync(dirname(dest), { recursive: true });
        const off = initSwitchOffs(f, readFileSync(src, "utf8"), p.switchedOff);
        if (off === undefined) cpSync(src, dest);
        else writeFileSync(dest, off);
        out(`forge: created ${f} (from ${pk.id})`);
      }
    }
  }
  return cmdApply(flags);
}

/**
 * An init template with the diagnostics active packs switch off (pack.yaml tool_off) set to
 * "off", the rest of the file as it is; undefined when none concerns it. Biome:
 * biome.json's linter.rules.<group>.<name>.
 */
function initSwitchOffs(file: string, text: string, offs: Plan["switchedOff"]): string | undefined {
  if (!/(^|\/)biome\.jsonc?$/.test(file)) return undefined;
  const mine = offs.filter((o) => o.tool === "biome");
  if (!mine.length) return undefined;
  let out = text;
  for (const o of mine) {
    const [, group, name] = o.expects.split("/");
    out = applyEdits(out, modify(out, ["linter", "rules", group!, name!], "off", { formattingOptions: { insertSpaces: true, tabSize: 2 } }));
  }
  return out;
}

// ---------------------------------------------------------------- main

export function main(argv: string[]): number {
  const flags: Flags = { user: false, local: false, force: false, dryRun: false, yes: false, json: false, preCommit: false };
  const args: string[] = [];
  for (const a of argv) {
    if (a === "--user") flags.user = true;
    else if (a === "--local") flags.local = true;
    else if (a === "--force") flags.force = true;
    else if (a === "--dry-run") flags.dryRun = true;
    else if (a === "--yes" || a === "-y") flags.yes = true;
    else if (a === "--json") flags.json = true;
    else if (a === "--pre-commit") flags.preCommit = true;
    else if (a === "--help" || a === "-h") args.unshift("help");
    else if (a === "--version" || a === "-v") args.unshift("version");
    else if (a.startsWith("-")) throw new ForgeError(`forge: unknown option ${a}\n\n${USAGE}`, 2);
    else args.push(a);
  }
  if (flags.user && flags.local) throw new ForgeError("forge: --user and --local cannot be combined", 2);
  const [cmd, ...rest] = args;
  switch (cmd) {
    case "init":
      return cmdInit(flags);
    case "apply":
      return cmdApply(flags);
    case "diff":
      return cmdDiff(flags);
    case "status":
      return cmdStatus(flags);
    case "explain":
      return cmdExplain(rest[0], flags);
    case "test":
      return cmdTest();
    case "remove":
      return cmdRemove(flags);
    case "version":
      out(FORGE_VERSION);
      return 0;
    case "help":
    case undefined:
      out(USAGE);
      return cmd ? 0 : 2;
    default:
      throw new ForgeError(`forge: unknown command ${cmd}\n\n${USAGE}`, 2);
  }
}

try {
  process.exitCode = main(process.argv.slice(2));
} catch (e) {
  if (e instanceof ForgeError) {
    err(e.message);
    process.exitCode = e.code;
  } else {
    err(`forge: ${(e as Error).message}`);
    process.exitCode = 1;
  }
}
