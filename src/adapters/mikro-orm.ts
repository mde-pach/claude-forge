/**
 * mikro-orm (@mikro-orm/cli, binary `mikro-orm`), the project's own (§16). What Forge asks of
 * it (typescript/mikro-orm/migration-present): `mikro-orm migration:up` then
 * `mikro-orm migration:check` on the local database.
 *
 * - `migration:check` reaches the CLI in 5.7.11 (CHANGELOG.md, 5.7.11, Bug Fixes: "**cli:**
 *   expose missing `migration:check` command (#4388)"). 5.6.5 had added the check (CHANGELOG.md
 *   5.6.5: "**cli:** add check for migrations command (#3923)") but the CLI did not register
 *   it: verified, 5.6.5 and 5.7.10 exit 1 `Unknown argument: migration:check`; 5.7.11 runs it.
 * - The oldest covered release is 5.0.0: 5 is the oldest major still in use, and
 *   `migration:up` runs the same from 5.6.4 to 7.2.5 (verified); 4.x is not covered.
 * - Exit statuses of `migration:check`: 0 `No changes required, schema is up-to-date`, 1
 *   `Changes detected. Please create migration to update schema.` (5.7.11, 5.9.8, 6.6.17,
 *   7.2.5). Unchanged through 6.x and 7.x for these two commands (7.0.0 / 6.6.9: "skip
 *   snapshot write on `migration:up`", 7.0.9: "skip database connection in checkSchema()
 *   when snapshot exists": no change to the invocation).
 * - Usage errors are yargs' (strict): exit 1 (as changes detected), the command's help, then
 *   `Unknown arguments: <names>` (or `Unknown argument: <name>`) as the last line.
 */
import type { ToolAdapter } from "./types.js";

export const MIKRO_ORM: ToolAdapter = {
  tool: "mikro-orm",
  verified: "7.2.5",
  usage: { exit: 1, pattern: "^Unknown arguments?: " },
  ranges: [
    {
      from: "5.0.0",
      to: "5.7.11",
      source: "CHANGELOG.md 5.7.11: cli: expose missing `migration:check` command (#4388)",
      ops: { migrate: "migration:up", "migrations-check": null },
    },
    {
      from: "5.7.11",
      source: "CHANGELOG.md 5.7.11: cli: expose missing `migration:check` command (#4388)",
      ops: { migrate: "migration:up", "migrations-check": "migration:check" },
    },
  ],
};
