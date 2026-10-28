/**
 * Redocly CLI, Forge's own pinned OpenAPI linter (§16.2 pinned tools): one range, the pin
 * (REDOCLY_VERSION), proven on @redocly/cli 2.62.1 through bunx. Forge's invocation (openapi.sh
 * forge_openapi_lint): `lint --format=github-actions <roots>`. Exit: 0 no error (warnings print),
 * 1 errors (`::error ` lines on stdout) and also a document it cannot parse or find and a usage
 * error (yargs), 2 a redocly.yaml it cannot load.
 */
import { REDOCLY_VERSION } from "../util.js";
import type { ToolAdapter } from "./types.js";

export const REDOCLY: ToolAdapter = {
  tool: "redocly",
  pinned: REDOCLY_VERSION,
  verified: REDOCLY_VERSION,
  // bunx @redocly/cli@<pin>
  runner: "forge_pinned redocly",
  usage: { exit: 1, pattern: "^(Unknown arguments?|Invalid values?):" },
  ranges: [
    {
      from: REDOCLY_VERSION,
      source: `@redocly/cli ${REDOCLY_VERSION}: https://github.com/Redocly/redocly-cli/releases/tag/%40redocly%2Fcli%40${REDOCLY_VERSION}; redocly lint --help`,
      ops: { lint: "lint --format=github-actions" },
    },
  ],
};
