/**
 * oasdiff, Forge's own pinned OpenAPI diff (§13.5 release binary, §16.2 pinned tools): one
 * range, the pin (RELEASE_BINARIES.oasdiff), proven on 1.33.0. Forge's invocation (openapi.sh
 * forge_oasdiff): `breaking <base> <revision> --fail-on ERR --format singleline`. Exit
 * (internal/errors.go): 0 no ERR-level change (warnings print, exit 0), 1 an ERR-level breaking
 * change (--fail-on ERR), 100 a usage error (cobra: unknown flag or command, invalid flag
 * value), 101 invalid flags, 102 a spec that cannot be loaded (missing, unparsable), 103-107 other
 * failures.
 */
import { RELEASE_BINARIES } from "../util.js";
import type { ToolAdapter } from "./types.js";

const PIN = RELEASE_BINARIES.oasdiff!.version;

export const OASDIFF: ToolAdapter = {
  tool: "oasdiff",
  pinned: PIN,
  verified: PIN,
  // forge_release_bin oasdiff
  runner: "forge_pinned oasdiff",
  usage: { exit: 100, pattern: "^Error: (unknown (shorthand )?flag|unknown command|invalid argument)" },
  ranges: [
    {
      from: PIN,
      source: `oasdiff ${PIN}: https://github.com/oasdiff/oasdiff/releases/tag/v${PIN}; internal/errors.go (exit codes 100-107)`,
      // caller: <base spec> <revision spec>
      ops: { diff: "breaking {args} --fail-on ERR --format singleline" },
    },
  ],
};
