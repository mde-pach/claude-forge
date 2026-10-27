### toolchain

Detected by `mise.toml`, `.mise.toml`, `.tool-versions`, `.devcontainer/**` or `.devcontainer.json`.

| Rule | Targets |
|---|---|
| `toolchain/pinned-versions` | `grep` forbid on `mise.toml`, `.mise.toml`, `mise.*.toml`, `.tool-versions`: a `latest` version |
| `toolchain/devcontainer-pinned` | `grep` forbid on `devcontainer.json`: an `"image"` with `:latest` or no tag or digest; `curl` / `wget` piped to `sh`, `bash` or `zsh` |
