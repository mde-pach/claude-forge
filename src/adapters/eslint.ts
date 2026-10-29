/**
 * ESLint, the project's own (§16), with a flat config (eslint.config.*): the ESLint 9 era.
 * What changed between its releases, for what Forge asks of it:
 *
 * - 9.0.0 is the oldest covered: flat config is the default from it (CHANGELOG.md v9.0.0:
 *   "feat!: Update shouldUseFlatConfig and CLI so flat config is default" #17748). ESLint 8
 *   reads a flat config only when one is present (eslint.config.js from 8.23.0, "Enable eslint.config.js lookup from CLI" #16235;
 *   eslint.config.mjs/.cjs from 8.57.0: "feat: Enable `eslint.config.mjs` and
 *   `eslint.config.cjs`" #18066) and otherwise reads .eslintrc*, where `--no-warn-ignored`
 *   (added in 8.51.0 for flat config only, #17569) is a usage error (`Invalid option
 *   '--warn-ignored' - perhaps you meant '--ignore'?`, exit 2; verified 8.57.1). 8.57.1 with
 *   an eslint.config.mjs runs every operation as 9 does, but which config an 8.x project
 *   reads depends on its files (and ESLINT_USE_FLAT_CONFIG), and typescript/eslint/*
 *   requires flat config: 8.x is not covered.
 * - 10.0.0 removed eslintrc altogether (CHANGELOG.md v10.0.0: "feat!: remove eslintrc
 *   support" #20037): the same command line (verified 10.0.0, 10.13.0). A project with only
 *   .eslintrc* crashes on 9 and 10 alike (`Oops! Something went wrong! :(`, exit 2: no
 *   usage error, a failure).
 * - Usage errors: exit 2 (as are crashes and config errors; findings exit 1) with
 *   `Invalid option '--x' - perhaps you meant ...` or `Invalid value for option 'x' - ...` on
 *   stderr, identical in 8.57.1-10.13.0.
 */
import type { ToolAdapter } from "./types.js";

export const ESLINT: ToolAdapter = {
  tool: "eslint",
  verified: "10.13.0",
  usage: { exit: 2, pattern: "^Invalid (option .--|value for option )" },
  ranges: [
    {
      from: "9.0.0",
      source: "eslint CHANGELOG.md v9.0.0: flat config is default (#17748); v10.0.0: remove eslintrc support (#20037)",
      ops: {
        // typescript/eslint/lint run (the project: `.`)
        lint: "--max-warnings=0",
        // typescript/eslint/lint changed_run (the changed files)
        "lint-files": "--max-warnings=0 --no-warn-ignored --no-error-on-unmatched-pattern",
        // the audit
        "lint-json": "--format json --no-warn-ignored",
      },
    },
  ],
};
