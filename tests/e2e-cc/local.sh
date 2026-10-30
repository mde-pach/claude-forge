#!/bin/sh
# Local convenience: runs run.sh under an interactive zsh, so a CLAUDE_CODE_OAUTH_TOKEN exported
# in ~/.zshrc is in run.sh's environment (it is then passed to the container by name only).
#   tests/e2e-cc/local.sh --bin <linux claude-forge> --image bench-tb4/cargo-flight-dispatch:1 [...]
here=$(cd "$(dirname "$0")" && pwd)
exec zsh -ic '"$0" "$@"' "$here/run.sh" "$@"
