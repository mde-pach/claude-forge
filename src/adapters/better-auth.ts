/**
 * better-auth, the CLI binary of the project's Better Auth install (§16). Which package
 * provides it: `@better-auth/cli` (bin `better-auth`, 1.0.0 to 1.4.22), then the npm package
 * `auth` from 1.5.0 (bins `better-auth`, and `auth` from 1.6.0). `auth` 1.0.0-1.2.0 is an
 * unrelated package with an `auth` bin only. `better-auth --version` prints the bare version
 * (commander `-V`). What changed, for what Forge asks of it:
 *
 * - 1.7.6 added the command (better-auth GitHub release v1.7.6, section `auth`, Features:
 *   "Added read-only `check` and `check schema` commands to validate your configured adapter
 *   schema on demand." #11314). Verified: `auth` 1.7.5 rejects it (`error: unknown option
 *   '--config'`, exit 1); @better-auth/cli 1.4.22 and auth 1.5.0 ship no `check` command
 *   (their dist registers generate, info, init, login, mcp, migrate, secret [, logout,
 *   upgrade]). The CLI's `check schema` code (checkSchemaAction: exit 0 passed, 1
 *   SCHEMA_MISMATCH, 2 could not run) is the same in 1.7.6 to 1.7.14; no release note from
 *   v1.7.7 to v1.7.14 mentions the schema check. So the first range is the releases the
 *   binary has and that cannot (before 1.7.6), the second from 1.7.6.
 * - Where it works, checked on the real CLI (2026-10-28, Node 24.3, each release of `auth` with the
 *   `better-auth` of the same release): on a SQLite database through Kysely (the live-database
 *   path) and on a Drizzle schema (the schema-file path), 1.7.6, 1.7.7, 1.7.8, 1.7.9, 1.7.10,
 *   1.7.11, 1.7.12, 1.7.13 and 1.7.14 all exit 1 on a fresh database (missing tables), 0 after
 *   `auth migrate` (or on the schema `auth generate` writes), 1 once a plugin adds fields the
 *   database or schema lacks (`username()`: drift), and reject an unknown option with
 *   commander's `error: unknown option`; 1.7.5 rejects `--config` (no `check` command). So the
 *   range starts at 1.7.6, the rule's only gate. The Prisma path was not run (it needs a
 *   generated client).
 * - Usage errors are commander's: exit 1 (as a schema mismatch) and a line starting with
 *   `error: ` (unknown option, unknown command, too many arguments, missing option argument).
 *   A mismatch prints `Database schema mismatch` and Better Auth's logger line (`ERROR`,
 *   upper case, after a timestamp), never a line starting with lower-case `error: `.
 */
import type { ToolAdapter } from "./types.js";

export const BETTER_AUTH: ToolAdapter = {
  tool: "better-auth",
  verified: "1.7.14",
  usage: { exit: 1, pattern: "^error: (unknown option|unknown command|too many arguments|option .* argument missing|missing required argument)" },
  ranges: [
    {
      from: "1.0.0",
      to: "1.7.6",
      source:
        "@better-auth/cli 1.x and auth 1.5.0-1.7.5 have no `check` command; github.com/better-auth/better-auth/releases/tag/v1.7.6 (auth: Added read-only `check` and `check schema` commands, #11314)",
      ops: { "schema-check": null },
    },
    {
      from: "1.7.6",
      source: "github.com/better-auth/better-auth/releases/tag/v1.7.6, `auth` Features: `check` and `check schema` commands (#11314)",
      // the caller's argument: the config file (file role @better-auth-config)
      ops: { "schema-check": "check schema --config {args}" },
    },
  ],
};
