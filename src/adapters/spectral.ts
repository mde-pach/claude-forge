/**
 * Spectral CLI, Forge's own pinned OpenAPI linter (§16.2 pinned tools): one range, the pin
 * (SPECTRAL_VERSION), proven on @stoplight/spectral-cli 6.17.0 through bunx. Forge's invocation
 * (openapi.sh forge_openapi_lint): `lint --ruleset <file> --format text --fail-severity error
 * <roots>`. Exit: 0 no result at or above error, 1 such results and also a usage error (yargs),
 * 2 a run that could not lint (no such document, an invalid ruleset).
 */
import { SPECTRAL_VERSION } from "../util.js";
import type { ToolAdapter } from "./types.js";

export const SPECTRAL: ToolAdapter = {
  tool: "spectral",
  pinned: SPECTRAL_VERSION,
  verified: SPECTRAL_VERSION,
  // bunx @stoplight/spectral-cli@<pin>
  runner: "forge_pinned spectral",
  usage: { exit: 1, pattern: "^(Unknown arguments?|Invalid values?):" },
  ranges: [
    {
      from: SPECTRAL_VERSION,
      source: `@stoplight/spectral-cli ${SPECTRAL_VERSION}: https://github.com/stoplightio/spectral/releases/tag/%40stoplight%2Fspectral-cli%40${SPECTRAL_VERSION}; spectral lint --help`,
      // caller: <ruleset> <roots>
      ops: { lint: "lint --format text --fail-severity error --ruleset" },
    },
  ],
};
