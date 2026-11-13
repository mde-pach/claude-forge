<!-- Generated from the built-in packs by crates/forge-packs/tests/spec_tables.rs; do not edit. Regenerate: FORGE_BLESS=1 cargo test -p forge-packs --test spec_tables -->

## 13. Packs and rules: `toolchain`

### toolchain

Tool versions pinned for every machine (mise, asdf .tool-versions, dev containers).

Detected by: a file matching `mise.toml`, `.mise.toml`, `.tool-versions`, `.devcontainer/**`, `.devcontainer.json`.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `toolchain/devcontainer-pinned` | devcontainer.json names an image with a version tag (or a digest), never latest or nothing, and its lifecycle commands do not pipe curl or wget into a shell. | any: `.devcontainer/devcontainer.json`, `.devcontainer/**/devcontainer.json`, `.devcontainer.json` | grep forbid `"image"[[:space:]]*:[[:space:]]*"([^"@:]*/)*[^"@:/]+(:latest)?"\|(curl\|wget)[^\|"]*\\|[[:space:]]*(sudo[[:space:]]+)?(ba\|z)?sh` |
| `toolchain/pinned-versions` | mise.toml, .mise.toml, mise.*.toml and .tool-versions pin versions, never latest: latest gives every machine and CI a different toolchain. | any: `mise.toml`, `.mise.toml`, `mise.*.toml`, `.tool-versions` | grep forbid `=[[:space:]]*["']latest["']\|^[A-Za-z0-9_-]+[[:space:]]+latest([[:space:]]\|$)` |
