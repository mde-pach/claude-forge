/**
 * npm, the project's package manager, found on PATH (§16). Forge asks one thing of it: is
 * package-lock.json in sync with the package.json files (`typescript/npm/lock-sync`), without
 * touching node_modules or running scripts. What changed between releases, for that:
 *
 * - 8.4.1 made `npm ci` validate the lock file against package.json (CHANGELOG v8.4.1:
 *   "fix(ci): lock file validation", #4363). Before, `npm ci --dry-run` on a stale lock
 *   exits 0 ("added 3 packages"): no check at all (verified on 7.24.2 and 8.0.0).
 * - 10.5.1 stopped `npm ci --dry-run` from deleting node_modules (CHANGELOG v10.5.1: "added
 *   check for dry-run", #7274). On 8.x, 9.x and 10.0.0 to 10.5.0 `ci --dry-run` removes the
 *   whole node_modules before it validates (verified: 8.4.1, 8.19.4, 9.9.4, 10.5.0 delete a
 *   marker file in node_modules; 10.5.1, 10.9.9, 11.21.1, 12.2.1 keep it). A lock check must
 *   not wipe the developer's install, so before 10.5.1 the lock is checked with
 *   `npm ls --package-lock-only --all` instead: it reads package.json and the lock only,
 *   offline, and exits 1 (ELSPROBLEMS: missing / invalid) exactly where `ci` refuses the lock
 *   (verified identical verdicts on 7.24.2, 8.0.0, 8.4.1, 9.9.4, 10.5.0, 10.5.1, 12.2.1 for
 *   an added dependency, a changed range, a removed dependency (both pass) and a clean lock).
 *   That also covers 8.0.0 to 8.4.0, where `ci` cannot.
 * - 12.0.0 rejects unknown options (CHANGELOG 12.0.0-pre.1: "error on unknown configs, flags,
 *   and abbreviations", #9276: exit 1, `npm error code EUNKNOWNCONFIG`). npm 8 to 10 ignore
 *   them silently (exit 0); npm 11 ignores them too, with
 *   `npm warn Unknown cli config "--x". This will stop working in the next major version of npm.`
 *   on stderr (11.21.1; 11.0.0 still silent).
 * - The oldest covered release is 8.0.0: npm 8 is the oldest major still shipped with a Node.js
 *   one may meet (Node 16); npm 7 behaves as 8.0.0 here but its Node lines are long dead.
 *
 * Usage error: exit 1 and EUNKNOWNCONFIG (npm 12) or `Unknown command:`. Not EUSAGE: npm
 * reports a stale lock as `npm error code EUSAGE` followed by the command's usage text.
 */
import type { ToolAdapter } from "./types.js";

export const NPM: ToolAdapter = {
  tool: "npm",
  runner: "path",
  verified: "12.2.1",
  usage: { exit: 1, pattern: "^(npm (error|ERR!) code EUNKNOWNCONFIG|Unknown command: )" },
  // npm as a command (any subcommand runs the project's npm), not the word in a message or a
  // file name (npm-scripts, `--type npm`)
  direct: "(^|[;&|`!{]|\\$\\(|\\b(?:then|do|else|exec|time)\\s)\\s*npm(?=$|[\\s;&|)`])",
  ranges: [
    {
      from: "8.0.0",
      to: "10.5.1",
      source: "npm CHANGELOG v8.4.1 (fix(ci): lock file validation, #4363); release/v10 CHANGELOG v10.5.1 (added check for dry-run, #7274): before 10.5.1 ci --dry-run deletes node_modules",
      ops: { "lock-check": "ls --package-lock-only --all" },
    },
    {
      from: "10.5.1",
      source: "release/v10 CHANGELOG v10.5.1 (added check for dry-run, #7274); CHANGELOG 12.0.0-pre.1 (#9276: unknown flags are errors)",
      ops: { "lock-check": "ci --dry-run --ignore-scripts --no-audit --no-fund" },
    },
  ],
};
