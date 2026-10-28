/**
 * Bun as the project's package manager and test runner, found on PATH (§16). Forge also runs
 * bun as its own JavaScript runtime (`forge_helper bun x.mjs`, `forge_ratchet bun …`,
 * `bun --no-install -e '…'`, `bun -e`, `bun <helper>.mjs`, `bunx <pinned tool>`): those stay
 * direct, so `direct` matches only bun's package-manager and test subcommands.
 *
 * What Forge asks of it: `lock-check` (typescript/bun/lock-sync: `bun install
 * --frozen-lockfile`) and `test-files` (typescript/bun/test: `bun test ./a.test.ts …`, the
 * paths prefixed with ./ so bun takes them as files, not name filters). What changed:
 *
 * - `bun install --frozen-lockfile` fails on a stale lock with `error: lockfile had changes,
 *   but lockfile is frozen` (exit 1) from 1.0.0 on, with the binary bun.lockb as with the text
 *   bun.lock (bun.com/blog/bun-v1.1.39: "bun.lock is bun's new text-based lockfile", behind
 *   `--save-text-lockfile`; bun.com/blog/bun-v1.2: "a new text-based lockfile: bun.lock", the
 *   default from 1.2.0, verified): the lock format changes nothing here. 1.0.0 is the oldest
 *   covered release (the first stable one).
 * - `bun test ./b.test.ts` ran nothing on 1.0.0 to 1.0.15 (`Ran 0 tests across 0 files.`,
 *   exit 0 even for a failing test: a false pass) and runs the file from 1.0.16 (verified
 *   1.0.0, 1.0.15 vs 1.0.16, 1.0.17). The 1.0.16 release notes (bun.sh/blog/bun-v1.0.16) do
 *   not name the fix: the boundary is the binaries'.
 * - 1.4.4 made package-manager commands reject unknown flags (bun.com/blog/bun-v1.4.4:
 *   "Package manager commands and bunx used to ignore unknown flags … error: bun install has no
 *   --frozen-lockfil flag."): exit 1 and that line. Before, `bun install` ignores them (exit 0,
 *   1.0.0–1.4.0 verified); `bun test` still ignores unknown options on 1.4.4, so a mistyped
 *   test option is never reported as a usage error.
 * - Not an invocation change, noted: with no lock file, `bun install --frozen-lockfile` fails on
 *   1.0.0 but creates the lock and installs (exit 0) on 1.1.38 and later.
 */
import type { ToolAdapter } from "./types.js";

export const BUN: ToolAdapter = {
  tool: "bun",
  runner: "path",
  verified: "1.4.4",
  usage: { exit: 1, pattern: "^error: bun [a-z]+ has no .* flag" },
  // bun's package-manager and test subcommands as a command; never `forge_helper bun x.mjs`,
  // `bun --no-install -e`, `bun -e`, `bun file.mjs`, bunx, or the words in a message
  direct:
    "(^|[;&|`!{]|\\$\\(|\\b(?:then|do|else|exec|time)\\s)\\s*bun\\s+(?:install|i|add|a|remove|rm|update|upgrade|outdated|link|unlink|pm|test|ci|audit|publish|patch|patch-commit|why|info)(?=$|[\\s;&|)`])",
  ranges: [
    {
      from: "1.0.0",
      to: "1.0.16",
      source: "bun 1.0.0–1.0.15 binaries: bun test ./file runs 0 files, exit 0 (release notes silent; fixed in 1.0.16, bun.sh/blog/bun-v1.0.16)",
      ops: { "lock-check": "install --frozen-lockfile", "test-files": null },
    },
    {
      from: "1.0.16",
      source: "bun 1.0.16 binary (bun test ./file runs it); bun.com/blog/bun-v1.4.4 (unknown flags are errors in package-manager commands)",
      ops: { "lock-check": "install --frozen-lockfile", "test-files": "test" },
    },
  ],
};
