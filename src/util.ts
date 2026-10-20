import { createHash } from "node:crypto";
import { spawnSync, type SpawnSyncOptions } from "node:child_process";
import { existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

export const FORGE_VERSION = "0.5.0";
export const CLAUDE_CODE_MIN = "2.1.295";
export const AST_GREP_VERSION = "0.45.3";

export function sha256(text: string | Buffer): string {
  return createHash("sha256").update(text).digest("hex");
}

/** Root of the installed package (contains packs/ and templates/). */
export function packageRoot(): string {
  let dir = dirname(fileURLToPath(import.meta.url));
  for (let i = 0; i < 4; i++) {
    if (existsSync(join(dir, "packs")) && existsSync(join(dir, "templates"))) return dir;
    dir = dirname(dir);
  }
  throw new Error("forge: cannot locate packs/ and templates/ next to the CLI");
}

export interface RunResult {
  status: number;
  stdout: string;
  stderr: string;
}

export function run(cmd: string, args: string[], opts: SpawnSyncOptions = {}): RunResult {
  const r = spawnSync(cmd, args, { encoding: "utf8", maxBuffer: 64 * 1024 * 1024, ...opts });
  return {
    status: r.status ?? (r.error ? 127 : 1),
    stdout: String(r.stdout ?? ""),
    stderr: String(r.stderr ?? ""),
  };
}

export function gitToplevel(cwd: string): string | undefined {
  const r = run("git", ["rev-parse", "--show-toplevel"], { cwd });
  return r.status === 0 ? r.stdout.trim() : undefined;
}

/** Tracked and untracked (not ignored) files, relative to the repo. */
export function listRepoFiles(repo: string): string[] {
  const r = run("git", ["ls-files", "-z", "--cached", "--others", "--exclude-standard"], { cwd: repo });
  if (r.status !== 0) return [];
  return [...new Set(r.stdout.split("\0").filter(Boolean))].filter((f) => existsSync(join(repo, f)));
}

export function compareVersions(a: string, b: string): number {
  const pa = a.split(".").map((x) => parseInt(x, 10) || 0);
  const pb = b.split(".").map((x) => parseInt(x, 10) || 0);
  for (let i = 0; i < 3; i++) if ((pa[i] ?? 0) !== (pb[i] ?? 0)) return (pa[i] ?? 0) - (pb[i] ?? 0);
  return 0;
}

export function shQuote(s: string): string {
  return `'${s.replace(/'/g, `'\\''`)}'`;
}
