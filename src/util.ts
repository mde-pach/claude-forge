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

/** Line diff (LCS) in unified style without hunks headers; "" when equal. */
export function lineDiff(before: string, after: string, label: string): string {
  if (before === after) return "";
  const a = before === "" ? [] : before.replace(/\n$/, "").split("\n");
  const b = after === "" ? [] : after.replace(/\n$/, "").split("\n");
  const n = a.length;
  const m = b.length;
  const out: string[] = [`--- ${label}`, `+++ ${label}`];
  if (n * m > 4_000_000) {
    for (const l of a) out.push(`-${l}`);
    for (const l of b) out.push(`+${l}`);
    return out.join("\n") + "\n";
  }
  const dp: Uint32Array[] = Array.from({ length: n + 1 }, () => new Uint32Array(m + 1));
  for (let i = n - 1; i >= 0; i--)
    for (let j = m - 1; j >= 0; j--) dp[i]![j] = a[i] === b[j] ? dp[i + 1]![j + 1]! + 1 : Math.max(dp[i + 1]![j]!, dp[i]![j + 1]!);
  const ops: string[] = [];
  let i = 0;
  let j = 0;
  while (i < n || j < m) {
    if (i < n && j < m && a[i] === b[j]) {
      ops.push(` ${a[i]}`);
      i++;
      j++;
    } else if (j < m && (i === n || dp[i]![j + 1]! >= dp[i + 1]![j]!)) ops.push(`+${b[j++]}`);
    else ops.push(`-${a[i++]}`);
  }
  // keep 2 lines of context around changes
  const keep = ops.map((o) => o[0] !== " ");
  for (let k = 0; k < ops.length; k++) if (ops[k]![0] !== " ") for (let d = -2; d <= 2; d++) if (ops[k + d] !== undefined) keep[k + d] = true;
  let skipped = false;
  ops.forEach((o, k) => {
    if (keep[k]) {
      out.push(o);
      skipped = false;
    } else if (!skipped) {
      out.push("@@");
      skipped = true;
    }
  });
  return out.join("\n") + "\n";
}
