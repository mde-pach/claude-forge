/**
 * sqlfluff, the project's own (§16). What changed between its releases, for what Forge asks of it
 * (sql/sqlfluff, forge_sqlfluff: `sqlfluff lint --format github-annotation-native --nocolor
 * <files>`, read as: exit 0 clean, 1 violations, anything else "could not run"):
 *
 * - 0.13.0 added the format (CHANGELOG.md 0.13.0: "Add native Github-actions output" #3107).
 *   But until 1.3.0 a run with violations exits 65, which forge_sqlfluff reports as "could not
 *   run" (verified: 0.13.0 and 1.2.1 exit 65 on a violation). 1.3.0 made the exit codes
 *   consistent (CHANGELOG.md 1.3.0: "Improved consistency of process exit codes", "Consistent
 *   return codes" #3608): 0 clean, 1 violations, 2 usage/errors. 1.3.0 is the oldest covered
 *   release.
 * - 3.0.0 broke the exit code of a directory run: `lint .` exits as its last file does (a
 *   directory holding bad.sql and clean.sql exits 0; explicit file lists are right), verified;
 *   fixed in 3.0.1 (CHANGELOG.md 3.0.1: "a hotfix to resolve a bug introduced
 *   affecting CLI exit codes in the 3.0.0 release", #5676). 3.0.0 cannot lint.
 * - The annotations themselves changed without changing what Forge reads (`::<level> ...
 *   file=..,line=..,col=..::CODE: message`): 3.0.0 made violations `::warning` instead of
 *   `::notice` (default `--annotation-level`), and added endLine/endColumn; 3.2.3 wraps each
 *   file in `::group::` / `::endgroup::` lines (#6312). Rule codes were renamed in 2.0.0 (L008
 *   -> LT01 ...): Forge expects none.
 * - Usage errors are click's: exit 2, `Error: No such option ...` after `Try 'sqlfluff lint
 *   --help' for help.` (`-h` with some click versions).
 */
import type { ToolAdapter } from "./types.js";

const LINT = "lint --format github-annotation-native --nocolor";

export const SQLFLUFF: ToolAdapter = {
  tool: "sqlfluff",
  verified: "4.4.0",
  usage: { exit: 2, pattern: "^Try .[^ ]+ [a-z-]+ (-h|--help). for help" },
  ranges: [
    {
      from: "1.3.0",
      to: "3.0.0",
      source: "CHANGELOG.md 1.3.0: Consistent return codes (#3608)",
      ops: { lint: LINT },
    },
    {
      from: "3.0.0",
      to: "3.0.1",
      source: "CHANGELOG.md 3.0.1: hotfix for the CLI exit codes broken in 3.0.0 (#5676)",
      ops: { lint: null },
    },
    {
      from: "3.0.1",
      source: "CHANGELOG.md 3.0.1 (#5676); 3.2.3 groups per file (#6312)",
      ops: { lint: LINT },
    },
  ],
};
