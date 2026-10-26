import { createHash } from "node:crypto";
import { spawnSync, type SpawnSyncOptions } from "node:child_process";
import { existsSync } from "node:fs";
import { availableParallelism } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

export const FORGE_VERSION = "0.5.0";
export const CLAUDE_CODE_MIN = "2.1.295";
export const AST_GREP_VERSION = "0.45.3";
/** Squawk (Postgres migration linter), run through bunx or uvx by migration-lint checks. */
export const SQUAWK_VERSION = "2.70.0";
/**
 * The Python types engine (§15.2), pinned like the other Forge tools (the newest release
 * older than 14 days): ty's language server through uvx on this Python. TypeScript's
 * engine is the project's own TypeScript (its tsserver, or tsc --lsp from 7 on).
 */
export const TY_VERSION = "0.0.86";
export const ENGINE_PYTHON = "3.12";
/** zizmor (GitHub Actions auditor), run through uvx by github-actions/zizmor. */
export const ZIZMOR_VERSION = "1.30.1";
/** hadolint, run through uvx from the hadolint-bin wheel, whose binary must match the upstream release. */
export const HADOLINT_VERSION = "2.15.1";
/**
 * sha256 of the upstream hadolint 2.15.1 release binaries
 * (https://github.com/hadolint/hadolint/releases/tag/v2.15.1), by `uname -s`-`uname -m`.
 */
export const HADOLINT_SHA256: Record<string, string> = {
  "Linux-x86_64": "c7187db94eeeeca956519a6af171adc31453941a1e777961f6e680f697c8c507",
  "Linux-aarch64": "f6198ef8090f404dbb771abfee086eb8c48ac177f30da7fd3510aca35b344b5d",
  "Linux-arm64": "f6198ef8090f404dbb771abfee086eb8c48ac177f30da7fd3510aca35b344b5d",
  "Darwin-arm64": "5c09f3213f8e40406abe048233d985eebef336d4a6a20021be47fadb6cf480a2",
  "Darwin-x86_64": "ffe9bb18b23d5ed1eae50237aecdbb523d016e96da0bd4e7aa432040acfc3fde",
};

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

/** A process for runParallel: env holds overrides of the current environment. */
export interface Job {
  cmd: string;
  args: string[];
  cwd?: string;
  env?: Record<string, string>;
  input?: string;
}

// The pool runParallel starts: reads the jobs (JSON) on stdin, runs <n> at a time, prints the results (JSON).
const POOL = `const { spawn } = require("node:child_process");
const jobs = JSON.parse(require("node:fs").readFileSync(0, "utf8"));
const n = Number(process.argv[process.argv.length - 1]) || 1;
const out = new Array(jobs.length);
let next = 0;
const one = (j) => new Promise((done) => {
  let stdout = "", stderr = "";
  const p = spawn(j.cmd, j.args, { cwd: j.cwd, env: { ...process.env, ...(j.env || {}) } });
  p.stdout.setEncoding("utf8").on("data", (d) => (stdout += d));
  p.stderr.setEncoding("utf8").on("data", (d) => (stderr += d));
  p.stdin.on("error", () => {});
  p.on("error", () => done({ status: 127, stdout, stderr }));
  p.on("close", (code) => done({ status: code ?? 1, stdout, stderr }));
  p.stdin.end(j.input || "");
});
Promise.all(Array.from({ length: Math.min(n, jobs.length) }, async () => {
  while (next < jobs.length) { const i = next++; out[i] = await one(jobs[i]); }
})).then(() => process.stdout.write(JSON.stringify(out)));
`;

/**
 * Runs independent processes <n> at a time and returns their results in job order, as
 * run() would one by one: synchronous for the caller, the processes overlap in a child pool.
 */
export function runParallel(jobs: Job[], n = availableParallelism()): RunResult[] {
  if (jobs.length < 2 || n < 2) return jobs.map((j) => run(j.cmd, j.args, { cwd: j.cwd, env: { ...process.env, ...j.env }, input: j.input ?? "" }));
  const r = spawnSync(process.execPath, ["-e", POOL, String(n)], { input: JSON.stringify(jobs), encoding: "utf8", maxBuffer: 512 * 1024 * 1024 });
  if (r.status !== 0) throw new Error(`forge: the process pool failed: ${String(r.stderr || r.error || "").trim()}`);
  return JSON.parse(r.stdout) as RunResult[];
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

/**
 * Hash of an owned file as the lock and .forge/manifest.sha256 record it. JSON
 * files are hashed with spaces, tabs and newlines removed, so a formatter run
 * is not tampering (check.sh does the same with `tr -d ' \t\r\n'`).
 */
/**
 * The hash of an owned file as check.sh computes it (forge_owned_hash): JSON without
 * whitespace; the helper scripts in .forge/helpers/ also without quotes, commas,
 * semicolons and parentheses, so a project formatter (ruff format, Biome) rewriting
 * them is not tampering (§12); anything else as it is.
 */
export function ownedHash(path: string, content: string): string {
  if (path.startsWith(".forge/helpers/")) return sha256(content.replace(/[ \t\r\n,;()'"`]/g, ""));
  return sha256(path.endsWith(".json") ? content.replace(/[ \t\r\n]/g, "") : content);
}
