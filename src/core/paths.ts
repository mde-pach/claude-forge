import type { Level } from "../types.js";

/**
 * Where Forge writes, by level (§4, §7, §8): the plugin, the lock and the manifest, the
 * settings files it appends to, and the owned paths a lock may name.
 */

/** The lock, relative to the base (the repository, or the home directory). */
export function lockPath(level: Level): string {
  return level === "user" ? ".claude/forge.lock.json" : ".forge/forge.lock.json";
}

/** check.sh's copy of the owned-file hashes (project level). */
export const MANIFEST_PATH = ".forge/manifest.sha256";

/** The settings file permission and sandbox entries go to. */
export function settingsFile(level: Level, local = false): string {
  return level === "project" && local ? ".claude/settings.local.json" : ".claude/settings.json";
}

/** The generated Claude Code plugin. */
export function pluginRoot(level: Level): string {
  return level === "user" ? ".claude/skills/forge-user" : ".claude/skills/forge-project";
}

/** Owned paths (§4) by level: a lock file entry must be one of them. */
export const OWNED: Record<Level, RegExp[]> = {
  project: [/^\.forge\/[^]+$/, /^\.claude\/skills\/forge-project\/[^]+$/, /^\.claude\/rules\/forge-[^/]+\.md$/],
  user: [/^\.claude\/skills\/forge-user\/[^]+$/],
};

/** The settings files each level may append to. */
export const SETTINGS_FILES: Record<Level, string[]> = { project: [".claude/settings.json", ".claude/settings.local.json"], user: [".claude/settings.json"] };
