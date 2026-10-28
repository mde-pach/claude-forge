/**
 * actionlint, Forge's own pinned GitHub Actions workflow checker (§13.5 release binary, §16.2
 * pinned tools): one range, the pin (RELEASE_BINARIES.actionlint), proven on 1.7.12. Forge's
 * invocation (github-actions.sh forge_actionlint): `-config-file <yaml> -shellcheck= -pyflakes=
 * -no-color -format '<template>' <files>`. forge_do splits the adapter's words on spaces, so the
 * template is written without spaces: `{{range.}}{{.Filepath}}:…{{"\x20"}}…` prints exactly what
 * Forge's `{{range $err := .}}{{$err.Filepath}}:{{$err.Line}}:{{$err.Column}}: …` does (checked
 * on 1.7.12; `{{range$err:=.}}` does not parse). Exit: 0 clean, 1 findings, 2 a usage error (Go
 * flag), 3 a fatal error (config, template, unreadable file): docs/usage.md, "Exit status".
 */
import { RELEASE_BINARIES } from "../util.js";
import type { ToolAdapter } from "./types.js";

const PIN = RELEASE_BINARIES["actionlint"]!.version;

export const ACTIONLINT: ToolAdapter = {
  tool: "actionlint",
  pinned: PIN,
  verified: PIN,
  // forge_release_bin actionlint: the sha256-checked binary in ~/.cache/forge/bin/actionlint-<pin>-<platform>/
  runner: "forge_pinned actionlint",
  usage: { exit: 2, pattern: "^(flag provided but not defined: |invalid (boolean )?value )" },
  ranges: [
    {
      from: PIN,
      source: `actionlint ${PIN}: https://github.com/rhysd/actionlint/releases/tag/v${PIN}; https://github.com/rhysd/actionlint/blob/v${PIN}/docs/usage.md (Exit status)`,
      ops: {
        // caller: <config yaml> <files>
        lint: '-shellcheck= -pyflakes= -no-color -format {{range.}}{{.Filepath}}:{{.Line}}:{{.Column}}:{{"\\x20"}}{{.Kind}}:{{"\\x20"}}{{.Message}}\\n{{end}} -config-file',
      },
    },
  ],
};
