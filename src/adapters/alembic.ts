/**
 * Alembic, the project's own (§16). What changed between its releases, for what Forge asks of it
 * (python/sqlalchemy/alembic: `alembic check`, `alembic heads`, and forge_squawk_alembic's
 * `alembic upgrade <down>:<rev> --sql`):
 *
 * - 1.5.0 added the `--version` option (alembic/config.py of the 1.5.0 sdist has it, 1.4.3's
 *   does not; the changelog does not mention it). Verified: 1.0.0 `alembic --version` exits 2
 *   `alembic: error: unrecognized arguments: --version`, 1.5.0 prints `alembic 1.5.0`. Forge
 *   asks the version with `--version`: 1.5.0 is the oldest covered release.
 * - 1.9.0 added `alembic check` (changelog.rst 1.9.0, feature, commands, #724: "Added new
 *   Alembic command `alembic check`"). Before it, `check` is an argparse invalid choice (exit
 *   2): `migrations-check` cannot. 1.13.0 fixed it for multi-env templates (#1234).
 * - `heads` and `upgrade <range> --sql` are the same on every covered release (1.5.0, 1.8.1,
 *   1.9.0, 1.13.0, 1.20.1).
 * - Exit statuses: 0 ok; 255 for a command's own failure (`FAILED: New upgrade operations
 *   detected: [...]`, `FAILED: Can't locate revision identified by 'zz'`); 2 for argparse usage
 *   errors (`alembic: error: unrecognized arguments: ...`, `... argument {...}: invalid choice`).
 */
import type { ToolAdapter } from "./types.js";

const COMMON = {
  heads: "heads",
  // the caller's argument: <down_revision>:<revision> (or <revision> for the first one)
  "upgrade-sql": "upgrade {args} --sql",
};

export const ALEMBIC: ToolAdapter = {
  tool: "alembic",
  verified: "1.20.1",
  usage: { exit: 2, pattern: "^[^ ]+: error: (unrecognized arguments|argument |the following arguments are required)" },
  ranges: [
    {
      from: "1.5.0",
      to: "1.9.0",
      source: "alembic 1.5.0 sdist alembic/config.py (--version option; not in the changelog); changelog.rst 1.9.0 #724 (alembic check)",
      ops: { ...COMMON, "migrations-check": null },
    },
    {
      from: "1.9.0",
      source: "alembic.sqlalchemy.org/en/latest/changelog.html 1.9.0, feature commands #724: Added new Alembic command alembic check",
      ops: { ...COMMON, "migrations-check": "check" },
    },
  ],
};
