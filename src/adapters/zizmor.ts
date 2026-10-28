/**
 * zizmor, Forge's own pinned GitHub Actions auditor (§16.2 pinned tools): one range, the pin
 * (ZIZMOR_VERSION), proven on 1.30.1 through uvx. Forge's invocation (github-actions.sh
 * forge_zizmor): `--offline --no-progress --no-ignores --format github <files>`. Exit: 0 clean,
 * 11-14 findings (the highest severity: 11 informational … 14 high), 1 an error (unreadable input, bad config),
 * 2 a usage error (clap), 3 no inputs collected (an unparsable workflow alone).
 */
import { ZIZMOR_VERSION } from "../util.js";
import type { ToolAdapter } from "./types.js";

export const ZIZMOR: ToolAdapter = {
  tool: "zizmor",
  pinned: ZIZMOR_VERSION,
  verified: ZIZMOR_VERSION,
  // uvx -q zizmor@<pin>
  runner: "forge_pinned zizmor",
  usage: { exit: 2, pattern: "^For more information, try .--help." },
  ranges: [
    {
      from: ZIZMOR_VERSION,
      source: `zizmor ${ZIZMOR_VERSION}: https://github.com/zizmorcore/zizmor/releases/tag/v${ZIZMOR_VERSION}; https://docs.zizmor.sh/usage/#exit-codes`,
      ops: { lint: "--offline --no-progress --no-ignores --format github" },
    },
  ],
};
