/**
 * editorconfig-checker, Forge's own pinned .editorconfig checker (§13.5 release binary, §16.2
 * pinned tools): one range, the pin (RELEASE_BINARIES["editorconfig-checker"]), proven on
 * 4.0.2. Forge's invocation (repository.sh forge_editorconfig): `-config <json> -no-color
 * -format gcc <files>`. Exit: 0 clean, 1 violations (and a file it cannot stat: no gcc line
 * then), 2 a usage error or an unparsable config.
 */
import { RELEASE_BINARIES } from "../util.js";
import type { ToolAdapter } from "./types.js";

const PIN = RELEASE_BINARIES["editorconfig-checker"]!.version;

export const EDITORCONFIG_CHECKER: ToolAdapter = {
  tool: "editorconfig-checker",
  pinned: PIN,
  verified: PIN,
  // forge_release_bin editorconfig-checker
  runner: "forge_pinned editorconfig-checker",
  usage: { exit: 2, pattern: "^(flag provided but not defined: |invalid (boolean )?value )" },
  ranges: [
    {
      from: PIN,
      source: `editorconfig-checker ${PIN}: https://github.com/editorconfig-checker/editorconfig-checker/releases/tag/v${PIN}; editorconfig-checker -help`,
      // caller: <config json> <files>
      ops: { lint: "-no-color -format gcc -config" },
    },
  ],
};
