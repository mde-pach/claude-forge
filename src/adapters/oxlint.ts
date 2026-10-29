/**
 * oxlint, the project's own (§16), with its type-aware rules run by oxlint-tsgolint. What
 * changed between its releases, for what Forge asks of it (`--type-aware --deny-warnings
 * --format unix`):
 *
 * - 1.11.0 added `--type-aware` (apps/oxlint/CHANGELOG.md 1.11.0: "oxlint: Add `tsgolint`
 *   integration" #12485); 1.10.0 rejects it (`Error: `--type-aware` is not expected in
 *   this context`, exit 1).
 * - 1.11.2 is the oldest covered: from it a type-aware run without oxlint-tsgolint fails
 *   (CHANGELOG.md 1.11.2: "linter/tsgolint: Report an error if tsgolint executable failed to
 *   spawn" #12984; verified: `Error running tsgolint: "Failed to spawn tsgolint ..."`, exit
 *   1). On 1.11.0 the same run exits 0 having run no type-aware rule, which is what
 *   typescript/oxlint/lint must never do (its type-aware rules skipped silently).
 * - 1.15.0 (#13590, "Report an error if the tsgolint exe could not be found": `Failed to find
 *   tsgolint executable`), 1.51.0 (#19614: `typeAware` also as a config option) and the
 *   tsgolint protocol changes (1.25.0 #15048, BREAKING; each oxlint declares the
 *   oxlint-tsgolint it needs as a peer: >=0.1.5 for 1.14, >=0.14.1 for 1.50, >=7.0.2005 for
 *   1.90) keep the command line (verified 1.13.0, 1.14.0, 1.15.0, 1.50.0, 1.90.0).
 * - Usage errors exit 1, like findings with --deny-warnings; bpaf's `Error: ...` line on
 *   stderr tells them apart (`Error: `--x` is not expected in this context`, `Error:
 *   couldn't parse `bogus`: 'bogus' is not a known format`). Findings print `path:l:c:
 *   message [Warning/...]` on stdout; a bad config prints `Failed to parse configuration
 *   file.`; neither starts with `Error: `.
 */
import type { ToolAdapter } from "./types.js";

export const OXLINT: ToolAdapter = {
  tool: "oxlint",
  verified: "1.90.0",
  usage: { exit: 1, pattern: "^Error: " },
  ranges: [
    {
      from: "1.11.2",
      source: "oxc apps/oxlint/CHANGELOG.md 1.11.0 (#12485 `--type-aware`); 1.11.2 (#12984 error when tsgolint cannot spawn)",
      ops: {
        // typescript/oxlint/lint: run (no argument: the project) and changed_run ({files})
        lint: "--type-aware --deny-warnings --format unix",
      },
    },
  ],
};
