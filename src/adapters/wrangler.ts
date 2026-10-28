/**
 * wrangler, the project's own (§16). What changed between its releases, for what Forge asks of it
 * (`wrangler types <file> --check`, typescript/cloudflare-workers/env-types-generated):
 *
 * - 4.59.0 added `--check` to `wrangler types` (packages/wrangler/CHANGELOG.md, 4.59.0, #11852:
 *   "Add `--check` flag to `wrangler types` command ... verify that your generated types file is
 *   up-to-date without regenerating it", with `wrangler types ./custom-types.d.ts --check`).
 *   Verified: 4.58.0, 4.0.0 and 3.114.17 reject it (`Unknown argument: check`, exit 1); 4.59.0
 *   passes on a fresh file, exits 1 `Types at <file> are out of date` after a binding is added.
 *   Later entries are fixes of the same check (4.84.1 #13572 `--env-interface` and secondary
 *   `--config`; 4.86.0 #13695 boolean flags; 4.89.0 #13751 `--env-file`; 4.99.0 #14034
 *   multi-worker), not boundaries of what Forge invokes.
 * - The oldest covered release is 3.0.0: wrangler 3 is the oldest major still in use, and
 *   `wrangler types [path]` takes the output path there already (3.114.17 verified); it cannot
 *   check. Wrangler 2 is not covered.
 * - The hash a types file records is the generating release's: a file written by 4.59.0 is
 *   "out of date" for 4.154.0 and the reverse (verified), so after a wrangler upgrade the check
 *   asks for a regeneration. That is a finding, as it should be.
 * - Usage errors are yargs': exit 1 (as a stale file) and `✘ [ERROR] Unknown argument(s): ...`,
 *   coloured even when piped (ANSI codes between `ERROR` and the message).
 */
import type { ToolAdapter } from "./types.js";

export const WRANGLER: ToolAdapter = {
  tool: "wrangler",
  verified: "4.154.0",
  usage: { exit: 1, pattern: "ERROR.*Unknown arguments?: " },
  ranges: [
    {
      from: "3.0.0",
      to: "4.59.0",
      source: "packages/wrangler/CHANGELOG.md 4.59.0 (#11852): `--check` added to `wrangler types`",
      ops: { "codegen-check": null },
    },
    {
      from: "4.59.0",
      source: "packages/wrangler/CHANGELOG.md 4.59.0 (#11852): Add `--check` flag to `wrangler types` command",
      // the caller's argument: the types file (file role @wrangler-types), one per call
      ops: { "codegen-check": "types {args} --check" },
    },
  ],
};
