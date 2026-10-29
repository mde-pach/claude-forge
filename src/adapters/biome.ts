/**
 * Biome, the project's own (§16). What changed between its releases, for what Forge asks of it:
 *
 * - 1.1.0 is the oldest covered: it is the first release that reads `// biome-ignore`
 *   (CHANGELOG_v1.md, 1.1.0: "You can use `// biome-ignore` as suppression comment."). 1.0.0
 *   only knows `// rome-ignore`, so a `biome-ignore` comment is no suppression there and
 *   typescript/no-unused-suppressions cannot be expressed (verified: 1.0.0 reports nothing on
 *   an unused `biome-ignore`, 1.1.0 reports `suppressions/unused`). `ci`, `check --apply`,
 *   `--no-errors-on-unmatched` (1.0.0: "Add the new option `--no-errors-on-unmatched`") and
 *   `format --stdin-file-path` all exist from 1.0.0.
 * - 1.7.0 added the JSON reporter (CHANGELOG_v1.md, 1.7.0: "Added two new options to customise
 *   the emitted output of the CLI: `--reporter=json` and `--reporter=json-pretty`"): before it
 *   the audit's `lint-json` cannot run.
 * - 1.8.0 added `--write` (CHANGELOG_v1.md, 1.8.0: "Add new options `--write`, `--fix` (alias
 *   of `--write`) and `--unsafe` to the command `biome lint` and `biome check`"), deprecating
 *   `--apply`, which 2.0.0 removed (packages/@biomejs/biome/CHANGELOG.md, 2.0.0: "Removed
 *   `--apply` and `--apply-unsafe`"). Before 1.8.0 `fix` is `check --apply`.
 * - 1.9.0 made `--max-diagnostics` accept `none` (CHANGELOG_v1.md, v1.9.0: "The option
 *   `--max-diagnostics` now accept a `none` value, which lifts the limit of diagnostics
 *   shown"). Before, `none` is a usage error and the largest value every older release
 *   accepts is 200 (1.0.x-1.1.x cap it at 200: "maximum accepted 200"; 1.7/1.8 at 65535), so
 *   `lint` lists at most 200 diagnostics there.
 * - Usage errors exit 1, like findings; the line tells them apart: 1.1.x prints the
 *   `flags/invalid` diagnostic ("Failed to parse CLI arguments."), later releases bpaf's
 *   `Error: ...` (unknown flag, bad value) on stderr. Findings and configuration errors never
 *   start a line with either.
 */
import type { ToolAdapter } from "./types.js";

/** What every covered release does the same way. */
const COMMON = {
  ci: "ci",
  "format-stdin": "format --stdin-file-path {args}",
};

export const BIOME: ToolAdapter = {
  tool: "biome",
  verified: "2.6.3",
  usage: { exit: 1, pattern: "^(flags/invalid|Error: )" },
  ranges: [
    {
      from: "1.1.0",
      to: "1.7.0",
      source: "CHANGELOG_v1.md 1.1.0 (`// biome-ignore`); 1.7.0 (`--reporter=json`)",
      ops: {
        ...COMMON,
        fix: "check --apply --no-errors-on-unmatched",
        lint: "lint --colors=off --max-diagnostics=200",
        "lint-json": null,
      },
    },
    {
      from: "1.7.0",
      to: "1.8.0",
      source: "CHANGELOG_v1.md 1.7.0: `--reporter=json` and `--reporter=json-pretty`",
      ops: {
        ...COMMON,
        fix: "check --apply --no-errors-on-unmatched",
        lint: "lint --colors=off --max-diagnostics=200",
        "lint-json": "lint --reporter=json --max-diagnostics=50",
      },
    },
    {
      from: "1.8.0",
      to: "1.9.0",
      source: "CHANGELOG_v1.md 1.8.0: new options `--write`, `--fix` and `--unsafe` (`--apply` deprecated, removed in 2.0.0)",
      ops: {
        ...COMMON,
        fix: "check --write --no-errors-on-unmatched",
        lint: "lint --colors=off --max-diagnostics=200",
        "lint-json": "lint --reporter=json --max-diagnostics=50",
      },
    },
    {
      from: "1.9.0",
      source: "CHANGELOG_v1.md v1.9.0: `--max-diagnostics` accepts `none`",
      ops: {
        ...COMMON,
        fix: "check --write --no-errors-on-unmatched",
        lint: "lint --colors=off --max-diagnostics=none",
        "lint-json": "lint --reporter=json --max-diagnostics=50",
      },
    },
  ],
};
