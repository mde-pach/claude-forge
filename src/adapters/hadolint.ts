/**
 * hadolint, Forge's own pinned Dockerfile linter (§16.2 pinned tools): one range, the pin
 * (HADOLINT_VERSION), proven on 2.15.1 (the hadolint-bin wheel through uvx; its binary's sha256
 * is the upstream release's, HADOLINT_SHA256). Forge's invocation (docker.sh forge_hadolint):
 * `-c <yaml> --disable-ignore-pragma --no-fail --no-color -f tty <files>`. With --no-fail the
 * exit is 0 whatever it finds; 1 is a usage error or an unreadable file; a config it cannot
 * parse is printed and ignored (exit 0).
 */
import { HADOLINT_VERSION } from "../util.js";
import type { ToolAdapter } from "./types.js";

export const HADOLINT: ToolAdapter = {
  tool: "hadolint",
  pinned: HADOLINT_VERSION,
  verified: HADOLINT_VERSION,
  // the hadolint binary of uvx --from hadolint-bin==<pin>, run once its sha256 matches the upstream release
  runner: "forge_pinned hadolint",
  usage: { exit: 1, pattern: "^(Invalid (option|argument) |option -.*: cannot parse value )" },
  ranges: [
    {
      from: HADOLINT_VERSION,
      source: `hadolint ${HADOLINT_VERSION}: https://github.com/hadolint/hadolint/releases/tag/v${HADOLINT_VERSION}; hadolint --help`,
      // caller: <yaml> <files>
      ops: { lint: "--disable-ignore-pragma --no-fail --no-color -f tty -c" },
    },
  ],
};
