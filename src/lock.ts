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

/**
 * Two-space indent, one `"path": "hash"` per line under `  "files": {`:
 * check.sh reads the files section with awk. The lock does not hash itself.
 */
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
