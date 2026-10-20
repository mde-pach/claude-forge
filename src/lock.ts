import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { CLAUDE_CODE_MIN, FORGE_VERSION } from "./util.js";
import type { Lock, SettingsEntry } from "./types.js";

export function readLock(base: string, lockPath: string): Lock | undefined {
  const f = join(base, lockPath);
  if (!existsSync(f)) return undefined;
  const data = JSON.parse(readFileSync(f, "utf8")) as Partial<Lock>;
  return {
    forge_version: data.forge_version ?? "",
    claude_code_min: data.claude_code_min ?? "",
    rules: data.rules ?? {},
    files: data.files ?? {},
    settings: data.settings ?? [],
  };
}

/** The lock, read by the CLI only (check.sh reads .forge/manifest.sha256). The lock does not hash itself. */
export function renderLock(rules: Record<string, string>, files: Record<string, string>, settings: SettingsEntry[]): string {
  const sorted = (o: Record<string, string>) => Object.fromEntries(Object.entries(o).sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0)));
  const lock: Lock = {
    forge_version: FORGE_VERSION,
    claude_code_min: CLAUDE_CODE_MIN,
    rules: sorted(rules),
    files: sorted(files),
    settings,
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
