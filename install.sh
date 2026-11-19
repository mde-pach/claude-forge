#!/bin/sh
# Installs Claude Forge: curl -LsSf https://github.com/mde-pach/claude-forge/releases/latest/download/install.sh | sh
#
# Downloads the release archive for this machine, checks it against the release's
# SHA256SUMS, and installs it as ~/.local/bin/claude-forge, plus a `forge` symlink unless
# another program named forge (Foundry's, say) is already on PATH.
#
# Settings (environment):
#   FORGE_VERSION      the version to install (default: the release this script came from)
#   FORGE_INSTALL_DIR  where to install (default: ~/.local/bin)
#   FORGE_DOWNLOAD_URL where releases are (default: GitHub releases); <url>/v<version>/<asset>
#
# To check the archive's build provenance as well: gh attestation verify <archive> --repo mde-pach/claude-forge
set -eu

# Replaced by the release workflow with the release's version.
default_version='@@FORGE_VERSION@@'
version=${FORGE_VERSION:-$default_version}
base=${FORGE_DOWNLOAD_URL:-https://github.com/mde-pach/claude-forge/releases/download}
dir=${FORGE_INSTALL_DIR:-${HOME:?HOME is not set}/.local/bin}

say() { printf 'claude-forge: %s\n' "$*" >&2; }
fail() { say "$*"; exit 1; }

case $version in
  @@*) fail "this install.sh has no version; set FORGE_VERSION (e.g. FORGE_VERSION=0.6.0)" ;;
  *[!0-9A-Za-z.+-]*|'') fail "invalid FORGE_VERSION: $version" ;;
esac

case $(uname -s) in
  Linux) os=unknown-linux-musl ;;
  Darwin) os=apple-darwin ;;
  *) fail "no release for $(uname -s); build from source: cargo install --git https://github.com/mde-pach/claude-forge forge-cli" ;;
esac
case $(uname -m) in
  x86_64|amd64) arch=x86_64 ;;
  aarch64|arm64) arch=aarch64 ;;
  *) fail "no release for $(uname -m)" ;;
esac
target=$arch-$os
asset=claude-forge-$version-$target.tar.gz

# HTTPS only, redirects included, unless FORGE_DOWNLOAD_URL names a plain-HTTP mirror.
case $base in
  http://*) proto='=http,https' ;;
  *) proto='=https' ;;
esac
if command -v curl >/dev/null 2>&1; then
  fetch() { curl --proto "$proto" --proto-redir "$proto" -fsSL --retry 2 --connect-timeout 15 -o "$2" "$1"; }
elif command -v wget >/dev/null 2>&1; then
  fetch() { wget -q -T 15 -O "$2" "$1"; }
else
  fail "curl or wget is needed"
fi
if command -v sha256sum >/dev/null 2>&1; then
  sha256() { sha256sum "$1" | cut -d' ' -f1; }
elif command -v shasum >/dev/null 2>&1; then
  sha256() { shasum -a 256 "$1" | cut -d' ' -f1; }
elif command -v openssl >/dev/null 2>&1; then
  sha256() { openssl dgst -sha256 -r "$1" | cut -d' ' -f1; }
else
  fail "sha256sum, shasum or openssl is needed to check the download"
fi

tmp=$(mktemp -d 2>/dev/null || mktemp -d -t claude-forge)
trap 'rm -rf "$tmp"' EXIT INT TERM

url=$base/v$version
say "downloading $asset"
fetch "$url/SHA256SUMS" "$tmp/SHA256SUMS" || fail "cannot download $url/SHA256SUMS"
fetch "$url/$asset" "$tmp/$asset" || fail "cannot download $url/$asset"
want=$(awk -v a="$asset" '{ n = $2; sub(/^\*/, "", n) } n == a { print $1; exit }' "$tmp/SHA256SUMS")
[ -n "$want" ] || fail "SHA256SUMS lists no $asset"
have=$(sha256 "$tmp/$asset")
[ "$have" = "$want" ] || fail "$asset does not match SHA256SUMS (expected $want, got $have)"

mkdir "$tmp/x"
(cd "$tmp/x" && tar -xzf "../$asset") || fail "cannot unpack $asset"
bin=$tmp/x/claude-forge-$version-$target/claude-forge
[ -f "$bin" ] || fail "$asset holds no claude-forge"

mkdir -p "$dir"
cp "$bin" "$dir/.claude-forge.tmp.$$"
chmod 755 "$dir/.claude-forge.tmp.$$"
mv -f "$dir/.claude-forge.tmp.$$" "$dir/claude-forge"
say "installed $dir/claude-forge ($version)"

# The `forge` name: ours unless another forge is already on PATH.
other=$(command -v forge 2>/dev/null || true)
if [ -n "$other" ] && [ "$other" != "$dir/forge" ]; then
  say "warning: $other is another program named forge, so no forge symlink was made; use claude-forge"
elif { [ -e "$dir/forge" ] || [ -L "$dir/forge" ]; } && [ "$(readlink "$dir/forge" 2>/dev/null || true)" != claude-forge ]; then
  say "warning: $dir/forge is another program, so it was left alone; use claude-forge"
else
  ln -sf claude-forge "$dir/forge"
  say "installed $dir/forge -> claude-forge"
fi

case :${PATH:-}: in
  *:"$dir":*) ;;
  *) say "add $dir to your PATH, e.g. in your shell profile: export PATH=\"$dir:\$PATH\"" ;;
esac
