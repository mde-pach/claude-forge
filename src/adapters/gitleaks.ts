/**
 * gitleaks, Forge's own pinned secret scanner (§13.5 release binary, §16.2 pinned tools): one
 * range, the pin (RELEASE_BINARIES.gitleaks), proven on 8.30.1. Forge's invocation
 * (repository.sh forge_gitleaks, from the copies' directory): `dir --no-banner --no-color
 * --redact -l error -c <toml> -i <ignore dir> --ignore-gitleaks-allow --exit-code 0
 * --report-format template --report-template <tmpl> --report-path <out> .`. With --exit-code 0
 * leaks exit 0 (the report file lists them); 1 is a fatal error (config, report format, bad
 * flag value, unknown command); 126 an unknown flag (cmd/root.go: `if strings.Contains(err.Error(),
 * "unknown flag") { os.Exit(126) }`).
 */
import { RELEASE_BINARIES } from "../util.js";
import type { ToolAdapter } from "./types.js";

const PIN = RELEASE_BINARIES["gitleaks"]!.version;

export const GITLEAKS: ToolAdapter = {
  tool: "gitleaks",
  pinned: PIN,
  verified: PIN,
  // forge_release_bin gitleaks
  runner: "forge_pinned gitleaks",
  usage: { exit: 126, pattern: "^Error: unknown (shorthand )?flag" },
  ranges: [
    {
      from: PIN,
      source: `gitleaks ${PIN}: https://github.com/gitleaks/gitleaks/releases/tag/v${PIN}; cmd/root.go (exit 126 on an unknown flag); gitleaks dir --help`,
      ops: {
        // caller: -c <toml> -i <ignore dir> --report-template <tmpl> --report-path <out> <dir>
        scan: "dir --no-banner --no-color --redact -l error --ignore-gitleaks-allow --exit-code 0 --report-format template {args}",
      },
    },
  ],
};
