/**
 * osv-scanner, Forge's own pinned vulnerability scanner (§13.5 release binary, §16.2 pinned
 * tools): one range, the pin (RELEASE_BINARIES["osv-scanner"]), proven on 2.7.0 (with
 * api.osv.dev reachable). Forge's invocation (supply-chain.sh forge_osv_scan): `scan source
 * --config <toml> --format markdown -L <lock>` (`requirements.txt:<path>` for a requirements*.txt).
 * Exit (docs/output.md, "Return Codes"): 0 packages, no vulnerability; 1 vulnerabilities; 127
 * a general error (unparsable or unknown lockfile, missing file, bad config, api.osv.dev
 * unreachable, and a usage error alike); 128 no packages found (a lockfile with no package).
 */
import { RELEASE_BINARIES } from "../util.js";
import type { ToolAdapter } from "./types.js";

const PIN = RELEASE_BINARIES["osv-scanner"]!.version;

export const OSV_SCANNER: ToolAdapter = {
  tool: "osv-scanner",
  pinned: PIN,
  verified: PIN,
  // forge_release_bin osv-scanner
  runner: "forge_pinned osv-scanner",
  usage: { exit: 127, pattern: "^Incorrect Usage: " },
  ranges: [
    {
      from: PIN,
      source: `osv-scanner ${PIN}: https://github.com/google/osv-scanner/releases/tag/v${PIN}; https://github.com/google/osv-scanner/blob/v${PIN}/docs/output.md (Return Codes)`,
      // caller: <config toml> -L <lock>
      ops: { scan: "scan source --format markdown --config" },
    },
  ],
};
