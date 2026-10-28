/**
 * Prisma CLI (`prisma`), the project's own (§16). What Forge asks of it: `prisma validate`
 * (typescript/prisma/validate) and `prisma migrate diff --from-migrations prisma/migrations
 * --to-schema prisma/schema.prisma --exit-code` (typescript/prisma/migration-present, run when
 * prisma.config.ts sets `shadowDatabaseUrl`).
 *
 * - 7.0.0 renamed the `migrate diff` source/target flags and moved the shadow database to the
 *   config file (GitHub release 7.0.0, "Removal of `prisma migrate` flags": "`--[from/to]-schema-
 *   datamodel` ... replaced with just `--[from/to]-schema`"; "Schema and config file updates":
 *   "`datasource.shadowDatabaseUrl` is now configured in the config file"). Before 7.0.0 the
 *   check as Forge writes it is a usage error (6.19.3: `unknown or unexpected option:
 *   --to-schema`), and the shadow database is a flag or a schema field Forge's rule does not
 *   read: `migrations-check` cannot. (4.16.2-6.19.3 do run `--to-schema-datamodel` on SQLite,
 *   verified; a range could use it if the rule learnt the pre-7 shadow URL.)
 * - `--exit-code` exists from 3.11.0 (release 3.11.0: "a new `--exit-code` flag to the
 *   `migrate diff` command"): 0 no difference, 2 difference, 1 error (verified 4.16.2-7.10.1).
 *   NOTE for the rule: today it fails on any non-zero exit, so an error (1, e.g. P3006 a
 *   migration that does not apply to the shadow database) is reported as a missing migration.
 * - The oldest covered release is 4.0.0, the oldest major still in use; `validate` runs the
 *   same from 4.16.2 to 7.10.1 (verified), and the Prisma pack only enables from 7.0.0.
 * - Usage errors: exit 1 (as an invalid schema) and `! unknown or unexpected option: --x`
 *   (`validate`) or `Error:` then `unknown or unexpected option: --x` (`migrate diff`). An
 *   invalid schema prints `Error code: P1012` and `error: Type ...`, never that line.
 */
import type { ToolAdapter } from "./types.js";

const MIGRATIONS_CHECK_7 = "migrate diff --from-migrations prisma/migrations --to-schema prisma/schema.prisma --exit-code";

export const PRISMA: ToolAdapter = {
  tool: "prisma",
  verified: "7.10.1",
  usage: { exit: 1, pattern: "^(! )?unknown or unexpected option: " },
  ranges: [
    {
      from: "4.0.0",
      to: "7.0.0",
      source: "github.com/prisma/prisma/releases/tag/7.0.0: Removal of `prisma migrate` flags (`--to-schema-datamodel` -> `--to-schema`); shadowDatabaseUrl moved to the config file",
      ops: { validate: "validate", "migrations-check": null },
    },
    {
      from: "7.0.0",
      source: "github.com/prisma/prisma/releases/tag/7.0.0: `--[from/to]-schema`; `datasource.shadowDatabaseUrl` in prisma.config.ts",
      ops: { validate: "validate", "migrations-check": MIGRATIONS_CHECK_7 },
    },
  ],
};
