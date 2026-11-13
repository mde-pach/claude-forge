#!/bin/sh
# End to end with a real Claude Code: Forge (this checkout's linux binary) applied in a container,
# then short `claude -p` sessions whose stream-json (hook events included) analyze.py checks.
#
#   tests/e2e-cc/run.sh --bin <linux claude-forge> [options] [scenario ...]
#
# Options:
#   --bin PATH          the claude-forge binary for the container's platform (required)
#   --image IMG         default: forge-e2e-cc (built from tests/e2e-cc/Dockerfile when missing);
#                       locally, bench-tb4/cargo-flight-dispatch:1
#   --platform P        docker --platform (default linux/amd64; on an arm Mac it is emulated)
#   --claude VERSION    Claude Code to run (default 2.1.312; "keep" keeps the image's if it is
#                       at least Forge's minimum)
#   --model M           default claude-haiku-5-5; the "realistic" scenario uses --real-model
#   --real-model M      default claude-sonnet-5-5
#   --prompts DIR       default tests/e2e-cc/prompts (a <name>.txt per scenario; DIR/<image
#                       basename>/ overrides per image)
#   --out DIR           where the transcripts land (default tests/e2e-cc/out/<timestamp>)
#   --keep              leave the container running
# Scenarios (default: hooks pip internals stopgate): hooks pip internals stopgate realistic
#
# Building the binary on macOS (arm or x86): cargo-zigbuild, about 2.5 min cold:
#   uv tool install cargo-zigbuild && uv tool install ziglang   # or pip install both
#   rustup target add x86_64-unknown-linux-musl
#   cargo zigbuild --release --locked --target x86_64-unknown-linux-musl -p forge-cli --bin claude-forge
#   -> target/x86_64-unknown-linux-musl/release/claude-forge (static, runs on any x86_64 Linux)
# On Linux: musl-tools and plain `cargo build --target x86_64-unknown-linux-musl`, as release.yml.
#
# Auth: CLAUDE_CODE_OAUTH_TOKEN must be in this process's environment; it is passed to the
# container by name only (docker exec -e NAME), never in argv or a file. Locally, local.sh runs
# this under `zsh -ic` for a token exported in ~/.zshrc. CI: .github/workflows/e2e-claude.yml.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
bin='' image=forge-e2e-cc platform=linux/amd64 claude=2.1.312
model=claude-haiku-5-5 real_model=claude-sonnet-5-5 prompts=$here/prompts out='' keep=0
while [ $# -gt 0 ]; do
  case $1 in
    --bin) bin=$2; shift 2 ;;
    --image) image=$2; shift 2 ;;
    --platform) platform=$2; shift 2 ;;
    --claude) claude=$2; shift 2 ;;
    --model) model=$2; shift 2 ;;
    --real-model) real_model=$2; shift 2 ;;
    --prompts) prompts=$2; shift 2 ;;
    --out) out=$2; shift 2 ;;
    --keep) keep=1; shift ;;
    -h | --help) sed -n '2,32p' "$0"; exit 0 ;;
    -*) echo "unknown option $1" >&2; exit 2 ;;
    *) break ;;
  esac
done
scenarios=${*:-hooks pip internals stopgate}
for s in $scenarios; do
  case $s in hooks | pip | internals | stopgate | realistic) ;; *) echo "unknown scenario $s" >&2; exit 2 ;; esac
done
[ -n "$bin" ] && [ -x "$bin" ] || { echo "--bin: an executable claude-forge is required" >&2; exit 2; }
[ -n "${CLAUDE_CODE_OAUTH_TOKEN:-}" ] || { echo "CLAUDE_CODE_OAUTH_TOKEN is not set" >&2; exit 2; }
out=${out:-$here/out/$(date +%Y%m%d-%H%M%S)}
mkdir -p "$out"
out=$(cd "$out" && pwd)

if [ "$image" = forge-e2e-cc ] && ! docker image inspect forge-e2e-cc >/dev/null 2>&1; then
  docker build -q --platform "$platform" -t forge-e2e-cc "$here" >/dev/null
fi
c=forge-e2e-cc-$$
cleanup() { [ "$keep" = 1 ] && echo "container kept: $c" || docker rm -f "$c" >/dev/null 2>&1 || true; }
trap cleanup EXIT INT TERM
docker run -d --name "$c" --platform "$platform" "$image" sleep infinity >/dev/null
docker cp -q "$bin" "$c:/usr/local/bin/claude-forge"
docker exec "$c" sh -c 'chmod 755 /usr/local/bin/claude-forge && ln -sf claude-forge /usr/local/bin/forge && mkdir -p /e2e/out /e2e/prompts'
docker cp -q "$here/in-container/." "$c:/e2e/"
for s in $scenarios; do
  p=$prompts/$(basename "$image" | cut -d: -f1)/$s.txt
  [ -f "$p" ] || p=$prompts/$s.txt
  docker cp -q "$p" "$c:/e2e/prompts/$s.txt"
done
docker exec -w /app "$c" sh /e2e/setup.sh "$claude"

for s in $scenarios; do
  m=$model turns=12
  [ "$s" = realistic ] && m=$real_model turns=60
  docker exec -e CLAUDE_CODE_OAUTH_TOKEN -e IS_SANDBOX=1 -w /app "$c" sh /e2e/session.sh "$s" "$m" "$turns"
  # each scenario starts from the applied baseline (the agent may have committed)
  docker exec -w /app "$c" sh -c 'git reset -q --hard "$(git log --format=%H --grep="^e2e baseline" -1)" && git clean -qfd'
done
# The rules files the agent reads, for the internals check.
docker exec -w /app "$c" sh -c 'cat .claude/rules/*.md 2>/dev/null' >"$out/rules.md" || true
docker cp -q "$c:/e2e/out/." "$out/"
# The token must not have landed in a transcript (printf is a builtin: it stays out of argv).
if printf '%s\n' "$CLAUDE_CODE_OAUTH_TOKEN" | grep -rqFf - "$out"; then
  rm -rf "$out"
  echo "the token appeared in a transcript: transcripts deleted" >&2
  exit 1
fi
echo "transcripts: $out"
# shellcheck disable=SC2086 # a list of names
python3 "$here/analyze.py" "$out" $scenarios
