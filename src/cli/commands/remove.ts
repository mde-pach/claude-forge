/** forge remove: delete exactly what the lock records. */
import { existsSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join } from "node:path";
import { readText } from "../../core/fs.js";
import { MANIFEST_PATH, lockPath } from "../../core/paths.js";
import { blockHash, hasBlock, onlyForge, removeBlock } from "../../precommit.js";
import { removeEntries } from "../../settings.js";
import { gitToplevel, ownedHash } from "../../util.js";
import type { Level, LockBlock } from "../../types.js";
import { lockOf } from "../context.js";
import type { Flags } from "../flags.js";
import { ForgeError, confirmSettings, deleteFile, err, out, pruneDirs, showDiff, writeSettingsFile } from "../io.js";

export function cmdRemove(flags: Flags): number {
  const level: Level = flags.user ? "user" : "project";
  const base = level === "user" ? homedir() : gitToplevel(process.cwd());
  if (!base) throw new ForgeError("forge: not inside a git repository");
  const lockFile = lockPath(level);
  const lock = lockOf(base, lockFile, level);
  if (!lock) throw new ForgeError(`forge: no ${lockFile}; nothing to remove`);
  const settings = new Map<string, { before: string; after?: string }>();
  for (const file of [...new Set(lock.settings.map((e) => e.file))]) {
    const before = readText(join(base, file));
    if (before === undefined) continue;
    settings.set(file, {
      before,
      after: removeEntries(
        before,
        lock.settings.filter((e) => e.file === file),
      ),
    });
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
  for (const f of stuck)
    err(
      `forge: ${f}: forge's pre-commit block cannot be removed safely (its begin/end markers were changed, or entries were added inside it); remove the lines between "# forge: begin" and "# forge: end" yourself`,
    );
  if (flags.dryRun) {
    for (const f of [...files, lockFile, ...(level === "project" && existsSync(join(base, MANIFEST_PATH)) ? [MANIFEST_PATH] : [])]) out(`--- ${f} (deleted)`);
    for (const [file, s] of settings) showDiff(s.before, s.after, file);
    for (const b of blocks) showDiff(b.before, b.after, b.block.file);
    return 0;
  }
  confirmSettings(
    [...settings].filter(([file, s]) => !file.endsWith("settings.local.json") && s.before !== (s.after ?? "")).map(([file, s]) => ({ file, ...s })),
    flags.yes,
    "forge: cancelled; nothing removed",
  );
  for (const f of files) {
    const text = readFileSync(join(base, f), "utf8");
    if (ownedHash(f, text) !== lock.files[f]) err(`forge: ${f} was modified since the last apply; removing it anyway`);
    deleteFile(base, f);
  }
  for (const [file, s] of settings) {
    writeSettingsFile(base, file, s.after);
    pruneDirs(dirname(join(base, file)), base);
  }
  for (const { block, before, after } of blocks) {
    if (blockHash(before) !== block.hash) err(`forge: ${block.file}: forge's pre-commit block was modified since the last apply; removing it anyway`);
    if (after === undefined) rmSync(join(base, block.file), { force: true });
    else writeFileSync(join(base, block.file), after);
  }
  rmSync(join(base, lockFile), { force: true });
  if (level === "project") rmSync(join(base, MANIFEST_PATH), { force: true });
  if (level === "project") {
    rmSync(join(base, ".forge", "state"), { recursive: true, force: true });
    pruneDirs(join(base, ".forge"), base);
  }
  pruneDirs(dirname(join(base, lockFile)), base);
  out(
    `forge: removed ${files.length} file(s) and ${lock.settings.length} settings entr${lock.settings.length === 1 ? "y" : "ies"}${blocks.length ? `, and the pre-commit block from ${blocks.map((b) => b.block.file).join(", ")}` : ""}`,
  );
  return stuck.length ? 1 : 0;
}
