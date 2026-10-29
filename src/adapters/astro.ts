/**
 * astro, the project's own (§16): `astro check` (diagnostics of .astro files, through the
 * Astro language server). The command line is the same on every covered release:
 *
 * - 1.0.0 is the oldest covered: the first stable release (packages/astro/CHANGELOG-v1.md,
 *   1.0.0), with `astro check` built in (verified on 1.0.0).
 * - 3.0.0 moved the checker out of `astro` (packages/astro/CHANGELOG-v3.md, 3.0.0, #8188:
 *   "The `astro check` command now requires an external package `@astrojs/check` and an
 *   install of `typescript` in your project"). Same command; without the package, 5.0.0 and
 *   7.4.2 print `[ERROR] [check] The `@astrojs/check` and `typescript` packages are
 *   required` and EXIT 0 under CI (nothing checked; outside CI they offer to install it), 3.0.0
 *   exits 1 (`Cannot read properties of null (reading 'name')`).
 * - 7.3.4 refuses TypeScript 7 (CHANGELOG.md 7.3.4, #18053: "astro check does not currently
 *   support TypeScript 7.0"): exit 1 with that error (verified 7.4.2 + TypeScript 7.0.2).
 * - No usage-error signature: astro's argument parser ignores unknown options (`astro check
 *   --definitely-not-an-option` checks as usual, exit 0/1) and an unknown command prints the
 *   help with exit 0, on every release run. A wrong command line can never be told from a
 *   result, so `usage` is null.
 */
import type { ToolAdapter } from "./types.js";

export const ASTRO: ToolAdapter = {
  tool: "astro",
  verified: "7.4.2",
  usage: null,
  ranges: [
    {
      from: "1.0.0",
      source: "withastro/astro packages/astro/CHANGELOG-v1.md 1.0.0; CHANGELOG-v3.md 3.0.0 (#8188: astro check needs @astrojs/check and typescript)",
      ops: { check: "check" },
    },
  ],
};
