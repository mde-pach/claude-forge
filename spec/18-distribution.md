## 18. Distribution

Forge is one Rust binary. It is built from the `forge-cli` crate of the workspace and installed under two names, `claude-forge` and `forge`, which run the same program. Hook entries call `claude-forge` (§7), so a machine whose `forge` is another program (Foundry's) still runs Forge's hooks.

### 18.1 Release

A tag `v<version>` runs the release workflow (`.github/workflows/release.yml`). The tag must equal the workspace version (`Cargo.toml` `[workspace.package] version`); otherwise the release stops. Each publishing job waits for an approval in its GitHub environment (`release`, `pypi`, `npm`).

| Target | Archive | Wheel |
|---|---|---|
| `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl` | yes (a static binary, checked) | musllinux_1_2 |
| `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu` | no | manylinux_2_17 (linked with zig) |
| `aarch64-apple-darwin`, `x86_64-apple-darwin` | yes | macOS |

- **GitHub release.** One `claude-forge-<version>-<target>.tar.gz` per archive target, holding `claude-forge-<version>-<target>/claude-forge`; `install.sh` with its version filled in; `SHA256SUMS`, listing every archive and `install.sh`; and GitHub build-provenance attestations for the archives and `install.sh`. A version with a `-` (a pre-release) is published as a pre-release.
- **PyPI** (`claude-forge`): six platform wheels built by maturin (`bindings = "bin"`), shipping both executables, published by trusted publishing. `uv tool install claude-forge` or `pipx install claude-forge` installs it.
- **npm** (`@maximedepachtere/claude-forge`): one package per platform (`-darwin-arm64`, `-darwin-x64`, `-linux-arm64`, `-linux-x64`, each with `os` and `cpu` set, holding that platform's binary), published first, then the main package. The main package's `claude-forge` and `forge` commands are one Node script (Node ≥ 18) that runs the platform package's binary with the same arguments, stdio and exit status. A platform without a package, or an install that omitted optional dependencies, fails with a message naming the fix. A pre-release is tagged `next`, a release `latest`. Published with provenance.
- **From source.** `cargo install --git https://github.com/mde-pach/claude-forge forge-cli` builds both executables. The crates are not published to crates.io (`publish = false`).

The toolchain is pinned in `rust-toolchain.toml`.

### 18.2 install.sh

`curl -LsSf https://github.com/mde-pach/claude-forge/releases/latest/download/install.sh | sh`:

1. The version is the release the script came from, or `FORGE_VERSION`. A version with characters outside `0-9A-Za-z.+-` is refused. A script with no version filled in asks for `FORGE_VERSION`.
2. The platform is `uname -s` (Linux: the musl build; Darwin) and `uname -m` (`x86_64`/`amd64`, `aarch64`/`arm64`). Any other platform is refused, with the `cargo install` line.
3. It downloads `SHA256SUMS` and the archive from `FORGE_DOWNLOAD_URL` (default GitHub releases; `<url>/v<version>/<asset>`) with curl or wget, over HTTPS only (redirects included) unless the URL is a plain-HTTP mirror. The archive's sha256 (sha256sum, shasum or openssl) must be its `SHA256SUMS` line; otherwise nothing is installed.
4. It installs `claude-forge` into `FORGE_INSTALL_DIR` (default `~/.local/bin`) through a temporary file and a rename, then a `forge` symlink to it, unless another program named `forge` is on `PATH` or already in that directory: then it warns and leaves `forge` alone.
5. It says when the directory is not on `PATH`.

It prints to stderr only, prefixed `claude-forge:`.

### 18.3 The version a repository runs

The installed binary is a launcher (§3). When `forge.lock` pins another Forge version, it runs `<cache>/bin/forge-<version>`, downloaded from the same GitHub release:

- The launcher downloads `SHA256SUMS`, then `claude-forge-<version>-<target>.tar.gz` for this platform (the musl build on Linux), checks the archive against its line, unpacks `claude-forge` and renames it to `<cache>/bin/forge-<version>`, under `<cache>/bin/.forge-<version>.lock`. A missing line or another sha256 installs nothing. Downloads follow §17.4.
- `forge apply` downloads it and waits. SessionStart starts the download in a detached process (its own process group, stdio on `/dev/null`, working directory `/`) and runs the installed version meanwhile; a download already holding the lock makes it exit at once. With `FORGE_NO_DOWNLOAD` set (§17), neither does. Every other command and hook never downloads it: the installed version runs, and a note for the person says so (`This project pins Forge <locked>, which is not downloaded yet, so Forge <running> ran instead. … Run `forge apply` to get it now.`). The SessionStart hook shows that note to the person as its `systemMessage`.
- What the checksum proves: both files come over HTTPS from the same release, so it catches a corrupt, truncated or mismatched download, not someone able to replace the release's assets. Forge does not require the build-provenance attestations, because checking one needs `gh` with a login or a Sigstore client; a person can check one with `gh attestation verify <archive> --repo mde-pach/claude-forge`.

### 18.4 A machine without Forge

The hook entries a repository commits must not break a teammate's session when Forge is not installed: each entry is `sh -c '<script>'` that runs `claude-forge hook <event>` only when `command -v claude-forge` finds it. Otherwise SessionStart prints `{"systemMessage":"Forge is not installed, so this project's checks are off. Install it with: curl -LsSf https://github.com/mde-pach/claude-forge/releases/latest/download/install.sh | sh"}` (a synchronous hook's `systemMessage` is shown to the person), and every other event exits 0 with no output, so the agent sees nothing (§7).

### Tests

- 18.1: `crates/forge-tools/tests/npm_package.rs` (the staged npm packages run the platform binary). (no test: the release workflow, the wheels and the attestations run only on a tag.)
- 18.2: `crates/forge-tools/tests/install_sh.rs` (installs `claude-forge` and `forge` under each of `sh`, `dash` and `bash` present; leaves another `forge` alone; refuses a bad checksum or no version).
- 18.3: `crates/forge-tools/tests/forge_fetch.rs` (verifies and installs; a wrong sum or a missing line installs nothing; a busy lock skips the background fetch; the background child), `crates/forge-tools/src/forge_release.rs` (unit: `SHA256SUMS` lines), `crates/forge-launcher/tests/exec.rs`, `crates/forge-launcher/src/policy.rs` (unit), `crates/forge-launcher/src/lib.rs` (unit: `FORGE_NO_DOWNLOAD`).
- 18.4: `crates/forge-launcher/tests/hook_shells.rs` (the wrapper under each shell, with and without `claude-forge`), `crates/forge-launcher/src/hook.rs` (unit: exact strings).
