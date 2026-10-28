/**
 * Trivy, Forge's own pinned misconfiguration scanner (§13.5 release binary, §16.2 pinned tools):
 * one range, the pin (RELEASE_BINARIES.trivy), proven on 0.75.0 with its bundled checks
 * (--skip-check-update). Forge's invocation (terraform.sh forge_trivy_config): `config --quiet
 * --cache-dir <dir> --misconfig-scanners terraform --severity HIGH,CRITICAL --skip-check-update
 * --ignorefile <file> --format template --template @<tpl> <dir>`. Without --exit-code findings
 * exit 0; 1 is a fatal error (usage, missing target or template), logged as "<time> FATAL Fatal
 * error <why>" (tab-separated). An unparsable .tf is not an error: no finding, exit 0.
 */
import { RELEASE_BINARIES } from "../util.js";
import type { ToolAdapter } from "./types.js";

const PIN = RELEASE_BINARIES["trivy"]!.version;

export const TRIVY: ToolAdapter = {
  tool: "trivy",
  pinned: PIN,
  verified: PIN,
  // forge_release_bin trivy
  runner: "forge_pinned trivy",
  usage: { exit: 1, pattern: "FATAL[[:space:]]+Fatal error[[:space:]]+(unknown (shorthand )?flag|unknown command|flag error)" },
  ranges: [
    {
      from: PIN,
      source: `trivy ${PIN}: https://github.com/aquasecurity/trivy/releases/tag/v${PIN}; trivy config --help`,
      ops: {
        // caller: --cache-dir <dir> --ignorefile <file> --template @<tpl> <dir>
        scan: "config --quiet --misconfig-scanners terraform --severity HIGH,CRITICAL --skip-check-update --format template {args}",
      },
    },
  ],
};
