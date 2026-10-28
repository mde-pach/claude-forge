/**
 * ast-grep, Forge's own pinned structural matcher (§16.2 pinned tools): its version is the pin
 * (AST_GREP_VERSION), so one range, proven on that release (ast-grep-cli through uvx and
 * @ast-grep/cli through bunx, both 0.45.3). Forge's invocations (`forge_do ast-grep scan|scan-meta`: check.sh
 * forge_counts / forge_forbid, post-edit.sh, tests.sh forge_test_blocks / snapshot values):
 * `scan -c <sgconfig> [--inline-rules <yaml>] --filter <re> --json=stream [--include-metadata] <files>`.
 * Exit: 0 no error-severity match (matches of other severities are still printed), 1 an
 * error-severity match, 2 a usage error (clap), 6 no readable sgconfig.
 */
import { AST_GREP_VERSION } from "../util.js";
import type { ToolAdapter } from "./types.js";

export const AST_GREP: ToolAdapter = {
  tool: "ast-grep",
  pinned: AST_GREP_VERSION,
  verified: AST_GREP_VERSION,
  // lib.sh's forge_pinned_ast_grep: bunx in bun repos, else uvx, retried --offline; it keeps stderr (the usage error)
  runner: "forge_pinned ast-grep",
  usage: { exit: 2, pattern: "^For more information, try .--help." },
  ranges: [
    {
      from: AST_GREP_VERSION,
      source: `ast-grep ${AST_GREP_VERSION}: https://github.com/ast-grep/ast-grep/releases/tag/${AST_GREP_VERSION}; ast-grep scan --help`,
      ops: {
        // caller: <sgconfig> [--inline-rules <yaml>] --filter <re> <files>
        scan: "scan --json=stream -c",
        "scan-meta": "scan --json=stream --include-metadata -c",
      },
    },
  ],
};
