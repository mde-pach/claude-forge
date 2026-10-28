/**
 * pnpm, the project's package manager, found on PATH (§16). Forge asks one thing of it: is
 * pnpm-lock.yaml in sync with the package.json files (`typescript/pnpm/lock-sync`), without
 * linking node_modules. The pnpm policy checks (allow-builds, catalog, overrides,
 * workspace-protocol, settings-location) read files with Forge's own helpers and never run
 * pnpm. What changed between releases, for that:
 *
 * - 8.6.12 made `--frozen-lockfile` hold under `--lockfile-only` (pnpm/CHANGELOG.md 8.6.12:
 *   "`pnpm install --frozen-lockfile --lockfile-only` should fail if the lockfile is not up to
 *   date with the `package.json` files", #6913). Before, the pair resolves again and REWRITES
 *   pnpm-lock.yaml, exit 0 (verified 8.6.11: stale lock → exit 0, lock changed; 8.0.0 also
 *   rewrites it). pnpm 7.4.1 had made that deliberate ("Never skip lockfile resolution when the
 *   lockfile is not up-to-date and `--lockfile-only` is used. Even if `frozen-lockfile` is
 *   `true`", #4951). No other pnpm 8 invocation checks the lock without installing, so older
 *   releases cannot.
 * - The oldest covered release is 8.0.0 (pnpm 7 and older are end of life and behave as 8.0.0
 *   here). pnpm 8.0.0–8.3.0 cannot reach the registry on Node.js 20+ (ERR_INVALID_THIS, fixed
 *   in 8.3.1, #6424); irrelevant since that range cannot lock-check anyway.
 * - Usage errors: 8.x–10.x print ` ERROR  Unknown option: 'x'` (thin spaces around ERROR),
 *   11.x `[ERROR] Unknown option: 'x'`, both exit 1; pnpm 12 (a native binary,
 *   @pnpm/exe.<platform>) uses clap: exit 2, `For more information, try '--help'.`. One exit
 *   status cannot cover 12 (see the report: ToolAdapter.usage needs a per-range status).
 */
import type { ToolAdapter } from "./types.js";

export const PNPM: ToolAdapter = {
  tool: "pnpm",
  runner: "path",
  verified: "12.15.0",
  // 8.x–11.x; pnpm 12's clap line is in the pattern but its exit status is 2 (unconfirmed how to express)
  usage: { exit: [1, 2], pattern: "(ERROR[^A-Za-z]{1,8}Unknown options?: |^For more information, try .--help.)" },
  // pnpm as a command, not the word in a message or a JS string ("pnpm" field checks)
  direct: "(^|[;&|`!{]|\\$\\(|\\b(?:then|do|else|exec|time)\\s)\\s*pnpm(?=$|[\\s;&|)`])",
  ranges: [
    {
      from: "8.0.0",
      to: "8.6.12",
      source: "pnpm/CHANGELOG.md 8.6.12 (#6913: --frozen-lockfile --lockfile-only fails on a stale lock); 7.4.1 (#4951)",
      ops: { "lock-check": null },
    },
    {
      from: "8.6.12",
      source: "pnpm/CHANGELOG.md 8.6.12 (#6913)",
      ops: { "lock-check": "install --frozen-lockfile --lockfile-only" },
    },
  ],
};
