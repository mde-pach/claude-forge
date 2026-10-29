/**
 * lockfile-lint, Forge's own pinned npm lockfile checker (§16.2 pinned tools): one range, the
 * pin (LOCKFILE_LINT_VERSION), proven on 5.1.0 through bunx. Forge's invocation (javascript.sh
 * forge_lockfile_lint): `--type npm --path <lock> --allowed-hosts npm --validate-https
 * --validate-integrity`. Exit: 0 clean (also a --path matching no file), 1 findings ("detected
 * …" on stderr) and also a lockfile it cannot parse or a bad --type. Unknown options are
 * ignored (yargs, not strict: exit 0), so the usage signature only covers a bad --type value.
 */
import { LOCKFILE_LINT_VERSION } from "../util.js";
import type { ToolAdapter } from "./types.js";

export const LOCKFILE_LINT: ToolAdapter = {
  tool: "lockfile-lint",
  pinned: LOCKFILE_LINT_VERSION,
  verified: LOCKFILE_LINT_VERSION,
  // bunx lockfile-lint@<pin>
  runner: "forge_pinned lockfile-lint",
  usage: { exit: 1, pattern: "^Unable to find relevant lockfile parser for type " },
  ranges: [
    {
      from: LOCKFILE_LINT_VERSION,
      source: `lockfile-lint ${LOCKFILE_LINT_VERSION}: https://github.com/lirantal/lockfile-lint/releases/tag/lockfile-lint%40${LOCKFILE_LINT_VERSION}; lockfile-lint --help`,
      // caller: <package-lock.json>
      ops: { lint: "--type npm --allowed-hosts npm --validate-https --validate-integrity --path" },
    },
  ],
};
