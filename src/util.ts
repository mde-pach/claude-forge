import { createHash } from "node:crypto";
import { spawnSync, type SpawnSyncOptions } from "node:child_process";
import { closeSync, existsSync, mkdtempSync, openSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { availableParallelism, tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

export const FORGE_VERSION = "0.5.0";
export const CLAUDE_CODE_MIN = "2.1.295";
export const AST_GREP_VERSION = "0.45.3";
/** Squawk (Postgres migration linter), run through bunx or uvx by migration-lint checks. */
export const SQUAWK_VERSION = "2.70.0";
/** lockfile-lint (package-lock.json hosts, https and integrity), run through bunx by typescript/npm/lockfile-lint. */
export const LOCKFILE_LINT_VERSION = "5.1.0";
/**
 * The Python types engine (§15.2), pinned like the other Forge tools (the newest release
 * older than 14 days): ty's language server through uvx on this Python. TypeScript's
 * engine is the project's own TypeScript (its tsserver, or tsc --lsp from 7 on).
 */
export const TY_VERSION = "0.0.86";
export const ENGINE_PYTHON = "3.12";
/** zizmor (GitHub Actions auditor), run through uvx by github-actions/zizmor. */
export const ZIZMOR_VERSION = "1.30.1";
/** Redocly CLI and Spectral CLI (OpenAPI linters), run through bunx by openapi/lint, the project's config choosing. */
export const REDOCLY_VERSION = "2.62.1";
export const SPECTRAL_VERSION = "6.17.0";
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

/**
 * Tools that ship only as GitHub release binaries (no PyPI or npm distribution), pinned
 * like the other Forge tools (the newest release older than 14 days). By `uname -s`-`uname -m`:
 * the release asset (a .tar.gz or .zip holding the binary, named like the tool, at its
 * root; or the binary itself) and its sha256, computed from the downloaded asset and
 * matching both GitHub's asset digest and the release's checksum file. check.sh fetches
 * an asset once, checks it, and keeps the binary under ~/.cache/forge/bin (§13.5).
 */
export interface ReleaseBinary {
  version: string;
  /** The release's download URL; the asset name is appended. */
  base: string;
  assets: Record<string, [asset: string, sha256: string]>;
}
export const RELEASE_BINARIES: Record<string, ReleaseBinary> = {
  "actionlint": {
    version: "1.7.12",
    base: "https://github.com/rhysd/actionlint/releases/download/v1.7.12/",
    assets: {
      "Linux-x86_64": ["actionlint_1.7.12_linux_amd64.tar.gz", "8aca8db96f1b94770f1b0d72b6dddcb1ebb8123cb3712530b08cc387b349a3d8"],
      "Linux-aarch64": ["actionlint_1.7.12_linux_arm64.tar.gz", "325e971b6ba9bfa504672e29be93c24981eeb1c07576d730e9f7c8805afff0c6"],
      "Linux-arm64": ["actionlint_1.7.12_linux_arm64.tar.gz", "325e971b6ba9bfa504672e29be93c24981eeb1c07576d730e9f7c8805afff0c6"],
      "Darwin-arm64": ["actionlint_1.7.12_darwin_arm64.tar.gz", "aba9ced2dee8d27fecca3dc7feb1a7f9a52caefa1eb46f3271ea66b6e0e6953f"],
      "Darwin-x86_64": ["actionlint_1.7.12_darwin_amd64.tar.gz", "5b44c3bc2255115c9b69e30efc0fecdf498fdb63c5d58e17084fd5f16324c644"],
    },
  },
  "gitleaks": {
    version: "8.30.1",
    base: "https://github.com/gitleaks/gitleaks/releases/download/v8.30.1/",
    assets: {
      "Linux-x86_64": ["gitleaks_8.30.1_linux_x64.tar.gz", "551f6fc83ea457d62a0d98237cbad105af8d557003051f41f3e7ca7b3f2470eb"],
      "Linux-aarch64": ["gitleaks_8.30.1_linux_arm64.tar.gz", "e4a487ee7ccd7d3a7f7ec08657610aa3606637dab924210b3aee62570fb4b080"],
      "Linux-arm64": ["gitleaks_8.30.1_linux_arm64.tar.gz", "e4a487ee7ccd7d3a7f7ec08657610aa3606637dab924210b3aee62570fb4b080"],
      "Darwin-arm64": ["gitleaks_8.30.1_darwin_arm64.tar.gz", "b40ab0ae55c505963e365f271a8d3846efbc170aa17f2607f13df610a9aeb6a5"],
      "Darwin-x86_64": ["gitleaks_8.30.1_darwin_x64.tar.gz", "dfe101a4db2255fc85120ac7f3d25e4342c3c20cf749f2c20a18081af1952709"],
    },
  },
  "kubeconform": {
    version: "0.8.0",
    base: "https://github.com/yannh/kubeconform/releases/download/v0.8.0/",
    assets: {
      "Linux-x86_64": ["kubeconform-linux-amd64.tar.gz", "9bc2bffbf71f261128533edaf912153948b7ff238f9a531ae6d34466ec287883"],
      "Linux-aarch64": ["kubeconform-linux-arm64.tar.gz", "1f53fc8e81258197a35e8603054162a5af1de8c5af13746c71ab680d9534ed87"],
      "Linux-arm64": ["kubeconform-linux-arm64.tar.gz", "1f53fc8e81258197a35e8603054162a5af1de8c5af13746c71ab680d9534ed87"],
      "Darwin-arm64": ["kubeconform-darwin-arm64.tar.gz", "f84f4dfbebf4a6b0b230385fa065a39ea35e02608c2b50d025dcf64775a69d67"],
      "Darwin-x86_64": ["kubeconform-darwin-amd64.tar.gz", "71dbc87ac9f24099a62b93570e65aa06312ba6ac8aea63b7f86e9d999edf5a92"],
    },
  },
  "kube-linter": {
    version: "0.8.3",
    base: "https://github.com/stackrox/kube-linter/releases/download/v0.8.3/",
    assets: {
      "Linux-x86_64": ["kube-linter-linux.tar.gz", "1a6d8419b11971372971fdbc22682b684ebfb7cf1c39591662d1b6ca736c41df"],
      "Linux-aarch64": ["kube-linter-linux_arm64.tar.gz", "802e1b09eabd08f6f0a060a6b8ab2bf7bc7e6bf4f673bb2692303704c84b3e22"],
      "Linux-arm64": ["kube-linter-linux_arm64.tar.gz", "802e1b09eabd08f6f0a060a6b8ab2bf7bc7e6bf4f673bb2692303704c84b3e22"],
      "Darwin-arm64": ["kube-linter-darwin_arm64.tar.gz", "6e3443a8ff8625a9fc31a38682c783988d7559018f7ff707a4f8c77c18c92f14"],
      "Darwin-x86_64": ["kube-linter-darwin.tar.gz", "c62e8af3c9df2557c7a3922119ea1b35597794737d1ccad493f63a0d66e7b8fc"],
    },
  },
  "tflint": {
    version: "0.64.0",
    base: "https://github.com/terraform-linters/tflint/releases/download/v0.64.0/",
    assets: {
      "Linux-x86_64": ["tflint_linux_amd64.zip", "cca9d13e2e1d7a2c627af60ff899a3c9b74212899416aeb96ec764d2ef954537"],
      "Linux-aarch64": ["tflint_linux_arm64.zip", "560da89aacf59389d4eb029730dd5b109b7288096c32f2726a0d9e783a5ea8eb"],
      "Linux-arm64": ["tflint_linux_arm64.zip", "560da89aacf59389d4eb029730dd5b109b7288096c32f2726a0d9e783a5ea8eb"],
      "Darwin-arm64": ["tflint_darwin_arm64.zip", "2496e9cb3d24992d553b45e7c87a0fdc9449ca975233876247a9bfeda857e6c0"],
      "Darwin-x86_64": ["tflint_darwin_amd64.zip", "0f3a9fd17526014646a2dfc3f9122f7b4161abe3d6b0f0f03f9014483ddf4d19"],
    },
  },
  "trivy": {
    version: "0.75.0",
    base: "https://github.com/aquasecurity/trivy/releases/download/v0.75.0/",
    assets: {
      "Linux-x86_64": ["trivy_0.75.0_Linux-64bit.tar.gz", "c6e65abddb348e25f10549df887045629cf28cc72453cd1c63acb717316b3f3f"],
      "Linux-aarch64": ["trivy_0.75.0_Linux-ARM64.tar.gz", "a1ee9f6ffb7d112b64ff726a2a0717c21175c1114361391f4a132956751a13b3"],
      "Linux-arm64": ["trivy_0.75.0_Linux-ARM64.tar.gz", "a1ee9f6ffb7d112b64ff726a2a0717c21175c1114361391f4a132956751a13b3"],
      "Darwin-arm64": ["trivy_0.75.0_macOS-ARM64.tar.gz", "4a77108cccf8e55c8d6823e1e759939a622277e66cd0daa3c1fc621ed69e4568"],
      "Darwin-x86_64": ["trivy_0.75.0_macOS-64bit.tar.gz", "291edaa9778acbe4693d067b5ad60ee11570e5ac68296e85595417528ca641e4"],
    },
  },
  "osv-scanner": {
    version: "2.7.0",
    base: "https://github.com/google/osv-scanner/releases/download/v2.7.0/",
    assets: {
      "Linux-x86_64": ["osv-scanner_linux_amd64", "160c6253e2922e9e0a6a9771c5c4c40a4b3c6090b34a3cdd5e80c96cc2e4218b"],
      "Linux-aarch64": ["osv-scanner_linux_arm64", "4061ef4c021ed3c47f203b818f337e351336cc5cd67c0274fc36bd1dfc728dd7"],
      "Linux-arm64": ["osv-scanner_linux_arm64", "4061ef4c021ed3c47f203b818f337e351336cc5cd67c0274fc36bd1dfc728dd7"],
      "Darwin-arm64": ["osv-scanner_darwin_arm64", "d4926ef8da134332543be276dff157e4c170f99ad91fc48e89448a8816bdf70b"],
      "Darwin-x86_64": ["osv-scanner_darwin_amd64", "e82c5df7ab0a42dff29a1cd0660711447b56b4c0483ff66ad8634df10fd81a45"],
    },
  },
  "oasdiff": {
    version: "1.33.0",
    base: "https://github.com/oasdiff/oasdiff/releases/download/v1.33.0/",
    assets: {
      "Linux-x86_64": ["oasdiff_1.33.0_linux_amd64.tar.gz", "43a4e328e2d13ba1552d760aa68d2485c75c5621f309f6ff64ae895188345247"],
      "Linux-aarch64": ["oasdiff_1.33.0_linux_arm64.tar.gz", "4ae3c362d6074d919aada2dea82d0ee84366591600384455d0bdc658ddf8f7ae"],
      "Linux-arm64": ["oasdiff_1.33.0_linux_arm64.tar.gz", "4ae3c362d6074d919aada2dea82d0ee84366591600384455d0bdc658ddf8f7ae"],
      "Darwin-arm64": ["oasdiff_1.33.0_darwin_all.tar.gz", "2a479337c15afdcbf0b1e89c0b4d0cf0176472dbc11483358fed1f831d1e46c5"],
      "Darwin-x86_64": ["oasdiff_1.33.0_darwin_all.tar.gz", "2a479337c15afdcbf0b1e89c0b4d0cf0176472dbc11483358fed1f831d1e46c5"],
    },
  },
  "editorconfig-checker": {
    version: "4.0.2",
    base: "https://github.com/editorconfig-checker/editorconfig-checker/releases/download/v4.0.2/",
    assets: {
      "Linux-x86_64": ["editorconfig-checker-linux-amd64.tar.gz", "44efaef3ed14c6ab52eb87481d78e5e5c204796dccf26e2f4598a61ba17f8003"],
      "Linux-aarch64": ["editorconfig-checker-linux-arm64.tar.gz", "abad76da8c9950fdc2a314bcd66969172aa823e9c595fa7e3a389a91e34d025e"],
      "Linux-arm64": ["editorconfig-checker-linux-arm64.tar.gz", "abad76da8c9950fdc2a314bcd66969172aa823e9c595fa7e3a389a91e34d025e"],
      "Darwin-arm64": ["editorconfig-checker-darwin-all.tar.gz", "ff59f57717d95fc62b8e372c8051dbbb766f7d660942409a85d027b0feb2422c"],
      "Darwin-x86_64": ["editorconfig-checker-darwin-all.tar.gz", "ff59f57717d95fc62b8e372c8051dbbb766f7d660942409a85d027b0feb2422c"],
    },
  },
};

/** The release table as check.sh reads it: "<name> <version> <platform> <url> <sha256>" per line. */
export function releaseTable(): string {
  return Object.entries(RELEASE_BINARIES)
    .flatMap(([name, r]) => Object.entries(r.assets).map(([plat, [asset, sha]]) => `${name} ${r.version} ${plat} ${r.base}${asset} ${sha}`))
    .join("\n");
}

/** tflint's provider rulesets (terraform/tflint), pinned like the tools; tflint --init fetches and verifies them. */
export const TFLINT_RULESETS: Record<string, string> = { aws: "0.49.0", google: "0.40.0", azurerm: "0.32.0" };

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

/**
 * spawnSync with no pipe at all: the input is read from a temporary file (none: /dev/null)
 * and the output written to temporary files. Under load, Bun's spawnSync (1.3) can return
 * a pipe's output empty although the process wrote it, and Node's (24) can leave a piped
 * input unwritten, the child waiting on its stdin for ever (seen with ruff
 * --stdin-filename during forge apply's audits). Forge's own test suite and `bun run
 * test:packs` run this code under Bun. env: process.env is Node's default; Bun's is the
 * environment the process started with.
 */
export function spawnCaptured(cmd: string, args: string[], opts: SpawnSyncOptions = {}): { status: number | null; error?: Error; stdout: string; stderr: string } {
  const dir = mkdtempSync(join(tmpdir(), "forge-run-"));
  const { input, ...rest } = opts;
  if (input !== undefined) writeFileSync(join(dir, "in"), input);
  const inp = input !== undefined ? openSync(join(dir, "in"), "r") : "ignore";
  const out = openSync(join(dir, "out"), "w");
  const err = openSync(join(dir, "err"), "w");
  try {
    const r = spawnSync(cmd, args, { env: process.env, ...rest, stdio: [inp, out, err] });
    return { status: r.status, error: r.error, stdout: readFileSync(join(dir, "out"), "utf8"), stderr: readFileSync(join(dir, "err"), "utf8") };
  } finally {
    if (typeof inp === "number") closeSync(inp);
    closeSync(out);
    closeSync(err);
    rmSync(dir, { recursive: true, force: true });
  }
}

export function run(cmd: string, args: string[], opts: SpawnSyncOptions = {}): RunResult {
  const r = spawnCaptured(cmd, args, opts);
  return { status: r.status ?? (r.error ? 127 : 1), stdout: r.stdout, stderr: r.stderr };
}

/**
 * A shell script run from a temporary file (`sh <file>`), never as `sh -c <script>`: Linux
 * caps one argument at 128 KiB (MAX_ARG_STRLEN), and lib.sh with engine.sh is larger.
 */
export function runScript(script: string, opts: SpawnSyncOptions = {}): RunResult {
  const dir = mkdtempSync(join(tmpdir(), "forge-script-"));
  try {
    writeFileSync(join(dir, "script.sh"), script);
    return run("sh", [join(dir, "script.sh")], opts);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
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
  const r = spawnCaptured(process.execPath, ["-e", POOL, String(n)], { input: JSON.stringify(jobs) });
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
