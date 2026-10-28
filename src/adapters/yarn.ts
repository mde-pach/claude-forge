/**
 * Yarn (Berry, 2 and later), the project's package manager, found on PATH (§16). Yarn 1 is
 * another tool: forge_has_tool treats it as absent (templates/engine/tools.sh), so no range
 * covers 0.x/1.x. Forge asks one thing of it: is yarn.lock in sync with the package.json files
 * (`typescript/yarn/lock-sync`), without running the packages' build scripts. The
 * scripts-ratchet check reads files (pm_policy.mjs) and never runs Yarn. What changed between
 * releases, for that (yarnpkg/berry CHANGELOG.md):
 *
 * - 2.0.0: `--immutable` (the new name of `--frozen-lockfile`) fails when the install would
 *   change the lock file ("will now properly report when the lockfile would be changed because
 *   of entry removals"). But there is no way to skip build scripts: `install --immutable` alone
 *   runs every dependency's postinstall, so 2.0.0 to 2.3.x cannot lock-check (2.3.3:
 *   `Unknown Syntax Error: Unsupported option name ("--skip-builds").`).
 * - 2.4.0 added `--skip-builds` ("A new `--skip-builds` option on `yarn install` will let you
 *   skip the build scripts without impacting the generated Yarn artifacts").
 * - 3.0.0 renamed it ("The `--skip-builds` flag from `yarn install` got renamed into
 *   `--mode=skip-build`"): 2.4.x rejects `--mode=skip-build` (`Invalid option name`), 3.0.0
 *   rejects `--skip-builds` (`Unsupported option name`).
 * - Nothing since changes the invocation (4.0.0, 4.18.1 verified).
 *
 * Usage error: exit 1 and clipanion's `Unknown Syntax Error: …` (unknown option, which 3.x/4.x
 * report as `Command not found; did you mean one of:`) or `Usage Error: …` (bad value: `Invalid
 * value for --mode`). Yarn colours these even when piped (`ESC[31mESC[1mUnknown Syntax
 * ErrorESC[22mESC[39m:`), so the pattern is not anchored and stops before the colon.
 */
import type { ToolAdapter } from "./types.js";

export const YARN: ToolAdapter = {
  tool: "yarn",
  runner: "path",
  verified: "4.18.1",
  usage: { exit: 1, pattern: "(Unknown Syntax Error|Usage Error)" },
  // yarn as a command, not the word in a message
  direct: "(^|[;&|`!{]|\\$\\(|\\b(?:then|do|else|exec|time)\\s)\\s*yarn(?=$|[\\s;&|)`])",
  ranges: [
    {
      from: "2.0.0",
      to: "2.4.0",
      source: "yarnpkg/berry CHANGELOG.md 2.4.0 (--skip-builds is new: before, an immutable install runs build scripts)",
      ops: { "lock-check": null },
    },
    {
      from: "2.4.0",
      to: "3.0.0",
      source: "yarnpkg/berry CHANGELOG.md 2.4.0 (new --skip-builds option on yarn install)",
      ops: { "lock-check": "install --immutable --skip-builds" },
    },
    {
      from: "3.0.0",
      source: "yarnpkg/berry CHANGELOG.md 3.0.0 (--skip-builds renamed into --mode=skip-build)",
      ops: { "lock-check": "install --immutable --mode=skip-build" },
    },
  ],
};
