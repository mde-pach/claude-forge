/**
 * Squawk, Forge's own pinned Postgres migration linter (§16.2 pinned tools): one range, the
 * pin (SQUAWK_VERSION), proven on squawk-cli 2.70.0 through bunx and uvx. Forge's invocation
 * (sql.sh forge_squawk): `--reporter gcc -c <toml> <files>` or `... --stdin-filepath <path>` with
 * the SQL on stdin. Exit: 0 clean, 1 any diagnostic (warnings included) and also a run that
 * could not lint (no such file, bad config: no gcc line then), 2 a usage error (clap).
 */
import { SQUAWK_VERSION } from "../util.js";
import type { ToolAdapter } from "./types.js";

export const SQUAWK: ToolAdapter = {
  tool: "squawk",
  pinned: SQUAWK_VERSION,
  verified: SQUAWK_VERSION,
  // bunx squawk-cli@<pin>, else uvx --from squawk-cli==<pin> squawk (sql.sh forge_squawk)
  runner: "forge_pinned squawk",
  usage: { exit: 2, pattern: "^For more information, try .--help." },
  ranges: [
    {
      from: SQUAWK_VERSION,
      source: `squawk ${SQUAWK_VERSION}: https://github.com/sbdchd/squawk/releases/tag/v${SQUAWK_VERSION}; squawk --help`,
      // caller: <toml> <files>, or <toml> --stdin-filepath <path> < sql
      ops: { lint: "--reporter gcc -c" },
    },
  ],
};
