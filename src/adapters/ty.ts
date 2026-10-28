/**
 * ty, Forge's own pinned type checker (§15.2, §16): its version is the pin, so one range,
 * proven by the spec tests on that release (ty's CLI: `ty check --output-format concise |
 * gitlab`, docs.astral.sh/ty/reference/cli/).
 */
import { TY_VERSION } from "../util.js";
import type { ToolAdapter } from "./types.js";

export const TY: ToolAdapter = {
  tool: "ty",
  pinned: TY_VERSION,
  verified: TY_VERSION,
  usage: { exit: 2, pattern: "^For more information, try .--help." },
  ranges: [
    {
      from: TY_VERSION,
      source: `ty ${TY_VERSION}: ty check --help`,
      ops: { typecheck: "check --output-format concise", "typecheck-gitlab": "check --output-format gitlab" },
    },
  ],
};
