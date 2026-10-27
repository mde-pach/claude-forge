import { existsSync, readFileSync, realpathSync } from "node:fs";
import { dirname, isAbsolute, join, relative } from "node:path";
import { hookManager, preCommitPath } from "./precommit.js";
import { validate } from "./schemas.js";
import { CLAUDE_CODE_MIN, FORGE_VERSION } from "./util.js";
import type { Level, Lock, LockBlock, SettingsEntry } from "./types.js";

/**
 * The lock, validated: `remove` deletes its files and blocks and rewrites its
 * settings files, and `apply` writes them, so a lock naming any other path (a
 * crafted or corrupted lock in a cloned repository) is refused as a whole.
 */
export function readLock(base: string, lockPath: string, level: Level): Lock | undefined {
  const f = join(base, lockPath);
  if (!existsSync(f)) return undefined;
  let data: unknown;
  try {
    data = JSON.parse(readFileSync(f, "utf8"));
  } catch (e) {
    throw new Error(`${lockPath}: not valid JSON (${(e as Error).message}); fix it or delete it`);
  }
  validate("lock", data, lockPath);
  const lock = data as Omit<Lock, "blocks"> & { blocks?: LockBlock[] };
  const bad = lockPathProblems(base, level, lock);
  if (bad.length) throw new Error(`${lockPath}: refused: ${bad.join("; ")}`);
  return { ...lock, blocks: lock.blocks ?? [] };
}

/** Owned paths (§4) by level: a lock file entry must be one of them. */
const OWNED: Record<Level, RegExp[]> = {
  project: [/^\.forge\/[^]+$/, /^\.claude\/skills\/forge-project\/[^]+$/, /^\.claude\/rules\/forge-[^/]+\.md$/],
  user: [/^\.claude\/skills\/forge-user\/[^]+$/],
};
const SETTINGS: Record<Level, string[]> = { project: [".claude/settings.json", ".claude/settings.local.json"], user: [".claude/settings.json"] };
/** The files hookManager() can name for each kind (the core.hooksPath one is asked of git). */
const BLOCK_FILES: Record<Exclude<LockBlock["kind"], "hooks">, string[]> = {
  "pre-commit": [".pre-commit-config.yaml"],
  husky: [".husky/pre-commit"],
  lefthook: ["lefthook.yml", "lefthook.yaml", ".lefthook.yml"],
};

/** A relative path without "." or ".." segments, empty segments or backslashes. */
export function plainRelative(p: string): boolean {
  return !isAbsolute(p) && !/[\\\0]/.test(p) && p.split("/").every((s) => s !== "" && s !== "." && s !== "..");
}

/** Whether base/p, with symbolic links resolved (its deepest existing ancestor when it does not exist), is inside base. */
export function resolvesInside(base: string, p: string): boolean {
  let real: string;
  try {
    const root = realpathSync(base);
    let at = join(base, p);
    const rest: string[] = [];
    while (!existsSync(at)) {
      rest.unshift(at.slice(dirname(at).length + 1));
      at = dirname(at);
    }
    real = join(realpathSync(at), ...rest);
    const rel = relative(root, real);
    return rel !== "" && !rel.startsWith("..") && !isAbsolute(rel);
  } catch {
    return false;
  }
}

function lockPathProblems(base: string, level: Level, lock: Omit<Lock, "blocks"> & { blocks?: LockBlock[] }): string[] {
  const out: string[] = [];
  // user level: ~/.claude may itself be a link (to a dotfiles repository); paths stay inside it
  const inside = (p: string) => plainRelative(p) && (level === "project" ? resolvesInside(base, p) : p.startsWith(".claude/") && resolvesInside(join(base, ".claude"), p.slice(".claude/".length)));
  // the hook Forge creates without a hook manager is git's own (in a worktree, outside the checkout)
  const hook = level === "project" ? preCommitPath(base) : undefined;
  for (const p of Object.keys(lock.files)) if (p !== hook && !(OWNED[level].some((re) => re.test(p)) && inside(p))) out.push(`files: ${JSON.stringify(p)} is not a path forge owns`);
  for (const s of lock.settings) if (!SETTINGS[level].includes(s.file) || !inside(s.file)) out.push(`settings: ${JSON.stringify(s.file)} is not a settings file forge appends to`);
  for (const b of lock.blocks ?? []) {
    const allowed = level === "project" && (b.kind === "hooks" ? hookManager(base)?.kind === "hooks" && hookManager(base)?.file === b.file : BLOCK_FILES[b.kind].includes(b.file));
    if (!allowed || !inside(b.file)) out.push(`blocks: ${JSON.stringify(b.file)} is not a ${b.kind} hook file forge adds its block to`);
  }
  return out;
}

/** The lock, read by the CLI only (check.sh reads .forge/manifest.sha256). The lock does not hash itself. */
export function renderLock(rules: Record<string, string>, files: Record<string, string>, settings: SettingsEntry[], blocks: LockBlock[] = [], roles: Record<string, string[]> = {}): string {
  const sorted = <T>(o: Record<string, T>) => Object.fromEntries(Object.entries(o).sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0)));
  const lock: Omit<Lock, "blocks"> & { blocks?: LockBlock[] } = {
    forge_version: FORGE_VERSION,
    claude_code_min: CLAUDE_CODE_MIN,
    rules: sorted(rules),
    files: sorted(files),
    settings,
    ...(blocks.length ? { blocks } : {}),
    ...(Object.keys(roles).length ? { roles: sorted(roles) } : {}),
  };
  return JSON.stringify(lock, null, 2) + "\n";
}

export const MANIFEST_PATH = ".forge/manifest.sha256";

/**
 * .forge/manifest.sha256: "<sha256>  <path>" per owned file under .forge/ and
 * .claude/, in sha256sum format, read by check.sh's integrity step. Hashes are
 * ownedHash() values (whitespace-insensitive for JSON).
 */
export function renderManifest(files: Record<string, string>): string {
  return (
    Object.entries(files)
      .filter(([p]) => p.startsWith(".forge/") || p.startsWith(".claude/"))
      .sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0))
      .map(([p, h]) => `${h}  ${p}`)
      .join("\n") + "\n"
  );
}
