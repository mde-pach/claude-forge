/**
 * kube-linter, Forge's own pinned Kubernetes manifest linter (§13.5 release binary, §16.2 pinned
 * tools): one range, the pin (RELEASE_BINARIES["kube-linter"]), proven on 0.8.3. Forge's
 * invocation (kubernetes.sh forge_kube_linter): `lint --config <yaml> --format plain
 * --with-color=false <files>`. Exit: 0 clean, 1 findings ("Error: found N lint errors" on
 * stderr) and 1 for every error too (bad config, unknown check, unknown flag, missing file):
 * only the stderr line tells a usage error. No `--version`: `kube-linter version`.
 */
import { RELEASE_BINARIES } from "../util.js";
import type { ToolAdapter } from "./types.js";

const PIN = RELEASE_BINARIES["kube-linter"]!.version;

export const KUBE_LINTER: ToolAdapter = {
  tool: "kube-linter",
  pinned: PIN,
  verified: PIN,
  // forge_release_bin kube-linter
  runner: "forge_pinned kube-linter",
  versionArgs: "version",
  usage: { exit: 1, pattern: "^Error: (unknown (shorthand )?flag|unknown command|invalid argument|invalid format)" },
  ranges: [
    {
      from: PIN,
      source: `kube-linter ${PIN}: https://github.com/stackrox/kube-linter/releases/tag/v${PIN}; kube-linter lint --help`,
      // caller: <config yaml> <files>
      ops: { lint: "lint --format plain --with-color=false --config" },
    },
  ],
};
