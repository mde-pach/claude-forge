/**
 * ruff, the project's own (§16). What changed between its releases, for what Forge asks of it:
 *
 * - 0.1.4 added `--output-format` to `ruff rule` (changelogs/0.1.x.md, 0.1.4: "Add
 *   `--output-format` to `ruff rule` and `ruff linter`", #8203): from then, the binary says
 *   which rules it has (`ruff rule --all --output-format json`: code, name, preview). Forge
 *   reads that and nothing older: 0.1.3 and earlier are not covered.
 * - 0.2.0 added the `concise` and `full` output formats and deprecated `text`
 *   (changelogs/0.2.x.md, 0.2.0, Deprecations: "`--output-format text`; use `full` or
 *   `concise` instead"). Before, `text` is the one-line format `concise` is now. 0.2.0 also
 *   moved the linter's settings under `[tool.ruff.lint]` (same section: "The following
 *   settings have moved and the previous name is deprecated"): before it, there is no current
 *   name to check the project's settings against.
 * - Rule codes come and go, and are renamed: TCH to TC in 0.8.0 (changelogs/0.8.x.md:
 *   "`flake8-type-checking`: `TCH` to `TC`"), flake8-trio merged into flake8-async in 0.5.0
 *   (TRIO100 is ASYNC100, the old ASYNC100 is ASYNC210). Their history is ruff's own rule
 *   tables (src/adapters/ruff-rules.ts, scripts/ruff-rules.ts).
 */
import type { ToolAdapter } from "./types.js";
import { RUFF_RULES } from "./ruff-rules.js";

/** The ruff releases the adapter and the rule history are proven on: each minor's first and last release, 0.1.4 to the newest. */
export const RUFF_MATRIX = [
  "0.1.4",
  "0.1.15",
  "0.2.0",
  "0.2.2",
  "0.3.0",
  "0.3.7",
  "0.4.0",
  "0.4.10",
  "0.5.0",
  "0.5.7",
  "0.6.0",
  "0.6.9",
  "0.7.0",
  "0.7.4",
  "0.8.0",
  "0.8.6",
  "0.9.0",
  "0.9.10",
  "0.10.0",
  "0.11.0",
  "0.11.13",
  "0.12.0",
  "0.12.12",
  "0.13.0",
  "0.13.3",
  "0.14.0",
  "0.14.14",
  "0.15.0",
  "0.15.22",
  "0.16.0",
  "0.16.10",
  "0.17.0",
  "0.17.3",
];

/** What every covered release does the same way. */
const COMMON = {
  "lint-json": "check --output-format json",
  "lint-json-stdin": "check --no-cache --no-fix --output-format json --stdin-filename {args} -",
  "format-check": "format --check",
  format: "format",
  "format-stdin": "format --stdin-filename {args} -",
  rules: "rule --all --output-format json",
};

export const RUFF: ToolAdapter = {
  tool: "ruff",
  verified: RUFF_MATRIX.at(-1)!,
  usage: { exit: 2, pattern: "^For more information, try .--help." },
  ranges: [
    {
      from: "0.1.4",
      to: "0.2.0",
      source: "changelogs/0.1.x.md 0.1.4 (#8203); changelogs/0.2.x.md 0.2.0 Deprecations",
      ops: {
        ...COMMON,
        lint: "check --output-format text",
        // after an edit: sorted imports and pyupgrade, when the project selects them; never F401 / F841
        fix: "check --fix --fixable I,UP --exit-zero --show-fixes --output-format text",
        "settings-warnings": null,
      },
    },
    {
      from: "0.2.0",
      source: "changelogs/0.2.x.md 0.2.0 (concise output format; settings moved under lint)",
      ops: {
        ...COMMON,
        lint: "check --output-format concise",
        fix: "check --fix --fixable I,UP --exit-zero --show-fixes --output-format concise",
        "settings-warnings": "check --exit-zero --no-cache",
      },
    },
  ],
  codes: { op: "rules", history: RUFF_RULES },
};
