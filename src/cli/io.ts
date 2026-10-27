/** Output, prompts and file writes the commands share. */
import { chmodSync, existsSync, mkdirSync, readdirSync, readSync, rmdirSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { lineDiff } from "../util.js";

export class ForgeError extends Error {
  constructor(
    message: string,
    readonly code = 1,
  ) {
    super(message);
  }
}

export const out = (s = "") => process.stdout.write(s + "\n");

export const err = (s: string) => process.stderr.write(s + "\n");

export function writeText(path: string, text: string): void {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, text);
  if (path.endsWith(".sh") || /\/hooks\/pre-commit$/.test(path)) chmodSync(path, 0o755);
}

/** Removes empty directories from dir up to (not including) stop. */
export function pruneDirs(dir: string, stop: string): void {
  while (dir.startsWith(stop + "/") && existsSync(dir) && readdirSync(dir).length === 0) {
    rmdirSync(dir);
    dir = dirname(dir);
  }
}

/** Asks a yes/no question on the terminal (`yes`: the answer is yes); without a terminal, fails with how to confirm. */
export function confirm(question: string, yes: boolean): boolean {
  if (yes) return true;
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

/** Prints the diff of a file: "(new)" when there was none, "(deleted)" when there is none after, unless a label is given. */
export function showDiff(before: string | undefined, after: string | undefined, path: string, label?: string): void {
  process.stdout.write(lineDiff(before ?? "", after ?? "", label ?? (before === undefined ? `${path} (new)` : after === undefined ? `${path} (deleted)` : path)));
}

/** Deletes base/rel and the directories it leaves empty (up to base). */
export function deleteFile(base: string, rel: string): void {
  rmSync(join(base, rel), { force: true });
  pruneDirs(dirname(join(base, rel)), base);
}

/** Writes a settings file, or deletes it when nothing is left in it (after: undefined). */
export function writeSettingsFile(base: string, file: string, after: string | undefined): void {
  if (after === undefined) rmSync(join(base, file), { force: true });
  else writeText(join(base, file), after);
}

/** Shows each change to a shared settings file and asks before it is written; a no cancels the command. */
export function confirmSettings(changes: { file: string; before: string; after?: string }[], yes: boolean, cancelled: string): void {
  for (const s of changes) {
    showDiff(s.before, s.after, s.file, s.file);
    if (!confirm(`forge: apply these changes to ${s.file}?`, yes)) throw new ForgeError(cancelled);
  }
}
