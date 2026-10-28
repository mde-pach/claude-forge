/**
 * tflint, Forge's own pinned Terraform linter (§13.5 release binary, §16.2 pinned tools), with
 * its pinned rulesets (tflint-ruleset-aws / -google / -azurerm, release binaries laid out under
 * TFLINT_PLUGIN_DIR): one range, the pin (RELEASE_BINARIES.tflint), proven on 0.64.0 (bundled
 * terraform ruleset 0.15.0) and tflint-ruleset-aws 0.49.0. Forge's invocation (terraform.sh
 * forge_tflint, TMPDIR=/tmp for the plugins' sockets): `--chdir <dir> --config <hcl> --format
 * compact --force`. With --force issues exit 0; 1 is an error: an HCL parse error (printed as
 * "path:l:c: error - …", lowercase), a plugin not found, a usage error ("Failed to parse CLI
 * options; …"). Without --force, issues exit 2.
 */
import { RELEASE_BINARIES } from "../util.js";
import type { ToolAdapter } from "./types.js";

const PIN = RELEASE_BINARIES["tflint"]!.version;

export const TFLINT: ToolAdapter = {
  tool: "tflint",
  pinned: PIN,
  verified: PIN,
  // forge_release_bin tflint, run with TMPDIR=/tmp and TFLINT_PLUGIN_DIR set by the caller (rulesets: forge_release_bin tflint-ruleset-<p>)
  runner: "forge_pinned tflint",
  usage: { exit: 1, pattern: "^Failed to parse CLI options; " },
  ranges: [
    {
      from: PIN,
      source: `tflint ${PIN}: https://github.com/terraform-linters/tflint/releases/tag/v${PIN}; tflint --help (--force: return zero exit status even if issues found)`,
      // caller: <dir> --config <hcl>
      ops: { lint: "--format compact --force --chdir" },
    },
  ],
};
