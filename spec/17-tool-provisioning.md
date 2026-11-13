## 17. Tool provisioning

Forge downloads what it runs into its per-user cache (§4): uv, the tools Forge pins, Bun when an npm-only tool needs it, and Forge's own releases (§18). It never installs anything into the repository or the project's environment. The project's own tools (ruff, Biome, tsc, pytest, …) are the project's: Forge finds them (§16) and never downloads them, except ruff: where the project does not configure it, Forge runs its pinned ruff with its own config, and where it configures ruff without installing it, with the project's config (§11.1).

### 17.1 Cache layout

Under the cache root (`$FORGE_HOME`, else `$XDG_CACHE_HOME/forge`, else `~/.cache/forge`):

| Path | Holds |
|---|---|
| `bin/forge-<version>` | Forge binaries the launcher runs (§3, §18) |
| `tools/<name>/<version>/` | one pinned tool (17.3) |
| `tools/.uv-cache/`, `tools/.python/`, `tools/.bun-cache/` | uv's cache, uv's managed Pythons, Bun's cache, shared by the installs |
| `path/` | only `uv` and `uvx`, symlinks to the pinned uv: the directory put on the agent's `PATH` (17.6) |
| `packs/` | the parsed pack cache |
| `engine/<repo-id>.sock` | the types engine's socket (§15.3) |

Version and tool names are checked before they become path components, so a value from a lock cannot escape its directory. `path/` holds nothing but `uv` and `uvx`: Forge removes any other entry, because everything there lands on the agent's `PATH`.

### 17.2 Pins

One table in forge-tools (`catalog.rs`) pins every tool Forge provisions. A pin is the newest release older than 14 days when it was taken. One exception: condor (PyPI `cndr`, Python dead code) is pinned at 0.3.0 the day of its release, because Forge's owner publishes Condor and vouches for that release; its catalog entry says so. `forge apply` writes the whole table into `forge.lock`'s `[tools]`, plus `python = "3.12"`, the Python the uv-installed tools run on (uv installs it as a managed Python, so it never depends on the machine's). A tool adapter that names a `pinned` version (§16) names this table's version: a test checks they agree.

`forge status` compares the lock's pins with the running Forge's: `Tool pins: as this Forge pins them`, or `Tool pins (forge.lock, then this Forge's; forge apply rewrites them):` and one line per difference (`--json`: `tool_pins`).

<!-- generated: tool pins (crates/forge-tools/tests/spec_pins.rs; FORGE_BLESS=1 rewrites it) -->
| Tool | Version | From | Checked against |
|---|---|---|---|
| uv | 0.13.1 | release asset, `https://github.com/astral-sh/uv/releases/download/0.13.1/` | pinned sha256 per platform: `Linux-x86_64`, `Linux-aarch64`, `Darwin-arm64`, `Darwin-x86_64` |
| bun | 1.4.3 | release asset, `https://github.com/oven-sh/bun/releases/download/bun-v1.4.3/` | pinned sha256 per platform: `Linux-x86_64`, `Linux-x86_64-musl`, `Linux-aarch64`, `Linux-aarch64-musl`, `Darwin-arm64`, `Darwin-x86_64` |
| ty | 0.0.86 | PyPI `ty`, installed by Forge's uv | the version only |
| serena | 2.1.0 | PyPI `serena-agent`, installed by Forge's uv | the version only |
| ruff | 0.17.1 | PyPI `ruff`, installed by Forge's uv | the version only |
| zizmor | 1.30.1 | PyPI `zizmor`, installed by Forge's uv | the version only |
| condor | 0.3.0 | PyPI `cndr`, installed by Forge's uv | the version only |
| hadolint | 2.15.1 | PyPI `hadolint-bin`, installed by Forge's uv | the upstream binary's sha256 per platform: `Linux-x86_64`, `Linux-aarch64`, `Darwin-arm64`, `Darwin-x86_64` |
| squawk | 2.70.0 | PyPI `squawk-cli`, installed by Forge's uv | the version only |
| ast-grep | 0.45.3 | PyPI `ast-grep-cli`, installed by Forge's uv | the version only |
| redocly | 2.62.1 | npm `@redocly/cli`, installed by Forge's Bun | the version only |
| spectral | 6.17.0 | npm `@stoplight/spectral-cli`, installed by Forge's Bun | the version only |
| lockfile-lint | 5.1.0 | npm `lockfile-lint`, installed by Forge's Bun | the version only |
| actionlint | 1.7.12 | release asset, `https://github.com/rhysd/actionlint/releases/download/v1.7.12/` | pinned sha256 per platform: `Linux-x86_64`, `Linux-aarch64`, `Darwin-arm64`, `Darwin-x86_64` |
| gitleaks | 8.30.1 | release asset, `https://github.com/gitleaks/gitleaks/releases/download/v8.30.1/` | pinned sha256 per platform: `Linux-x86_64`, `Linux-aarch64`, `Darwin-arm64`, `Darwin-x86_64` |
| kubeconform | 0.8.0 | release asset, `https://github.com/yannh/kubeconform/releases/download/v0.8.0/` | pinned sha256 per platform: `Linux-x86_64`, `Linux-aarch64`, `Darwin-arm64`, `Darwin-x86_64` |
| kube-linter | 0.8.3 | release asset, `https://github.com/stackrox/kube-linter/releases/download/v0.8.3/` | pinned sha256 per platform: `Linux-x86_64`, `Linux-aarch64`, `Darwin-arm64`, `Darwin-x86_64` |
| tflint | 0.64.0 | release asset, `https://github.com/terraform-linters/tflint/releases/download/v0.64.0/` | pinned sha256 per platform: `Linux-x86_64`, `Linux-aarch64`, `Darwin-arm64`, `Darwin-x86_64` |
| tflint-ruleset-aws | 0.49.0 | release asset, `https://github.com/terraform-linters/tflint-ruleset-aws/releases/download/v0.49.0/` | pinned sha256 per platform: `Linux-x86_64`, `Linux-aarch64`, `Darwin-arm64`, `Darwin-x86_64` |
| tflint-ruleset-google | 0.40.0 | release asset, `https://github.com/terraform-linters/tflint-ruleset-google/releases/download/v0.40.0/` | pinned sha256 per platform: `Linux-x86_64`, `Linux-aarch64`, `Darwin-arm64`, `Darwin-x86_64` |
| tflint-ruleset-azurerm | 0.32.0 | release asset, `https://github.com/terraform-linters/tflint-ruleset-azurerm/releases/download/v0.32.0/` | pinned sha256 per platform: `Linux-x86_64`, `Linux-aarch64`, `Darwin-arm64`, `Darwin-x86_64` |
| trivy | 0.75.0 | release asset, `https://github.com/aquasecurity/trivy/releases/download/v0.75.0/` | pinned sha256 per platform: `Linux-x86_64`, `Linux-aarch64`, `Darwin-arm64`, `Darwin-x86_64` |
| osv-scanner | 2.7.0 | release asset, `https://github.com/google/osv-scanner/releases/download/v2.7.0/` | pinned sha256 per platform: `Linux-x86_64`, `Linux-aarch64`, `Darwin-arm64`, `Darwin-x86_64` |
| oasdiff | 1.33.0 | release asset, `https://github.com/oasdiff/oasdiff/releases/download/v1.33.0/` | pinned sha256 per platform: `Linux-x86_64`, `Linux-aarch64`, `Darwin-arm64`, `Darwin-x86_64` |
| editorconfig-checker | 4.0.2 | release asset, `https://github.com/editorconfig-checker/editorconfig-checker/releases/download/v4.0.2/` | pinned sha256 per platform: `Linux-x86_64`, `Linux-aarch64`, `Darwin-arm64`, `Darwin-x86_64` |
| python | 3.12 | a uv-managed Python, for the PyPI tools | uv's own checks |
<!-- end generated -->

Platform keys are `uname -s`-`uname -m`: `Linux-x86_64`, `Linux-aarch64`, `Darwin-arm64`, `Darwin-x86_64`. On musl Linux (a `/lib/ld-musl-*` loader) the `-musl` key is looked up first; only Bun has musl builds. A platform without an asset gets the note `Forge has no pinned <tool> <version> for <platform>; the checks that use it are skipped.`

### 17.3 Installing a tool

Every install runs under a lock file of the tool version (`tools/<name>/.<version>.lock`), and checks again once it holds the lock, so concurrent processes download once.

- **Release asset** (uv, Bun, and the release binaries in 17.2). Forge downloads the platform's asset into a temporary sibling of the tool directory and computes its sha256 while it streams. A sha256 other than the pinned one installs nothing: `The downloaded <tool> <version> (<url>) does not match its pinned sha256 <want> (got <got>), so it was not installed. The checks that use it are skipped.` Forge then extracts only the named executables (`uv` and `uvx`, `bun`, or the tool's binary) from the `.tar.gz` or `.zip` (or takes a raw binary as is) and renames the directory into place, so `tools/<name>/<version>/` exists only when complete.
- **PyPI package** (ty, ruff, zizmor, condor, hadolint, squawk, ast-grep, serena). Forge's uv runs `uv tool install --quiet --managed-python --python 3.12 <package>==<version>` from the tool directory, so no project's `uv.toml` or `pyproject.toml` applies, with `UV_TOOL_DIR=<dir>/env`, `UV_TOOL_BIN_DIR=<dir>/bin`, the shared uv cache and Python directory, and `VIRTUAL_ENV`, `UV_PYTHON` and `UV_SYSTEM_PYTHON` removed. The executable is `<dir>/bin/<binary>`. A wheel that repackages an upstream binary (hadolint's `hadolint-bin`) must hold that binary: its sha256 is compared with the upstream release's per platform, and a mismatch removes the install. The package itself comes from PyPI over HTTPS through uv; Forge pins its version, not its hash.
- **npm package** (redocly, spectral, lockfile-lint). Forge first provisions its pinned Bun, writes a `package.json` with the exact version, runs `bun install --production --ignore-scripts`, and writes `<dir>/bin/<binary>`, a script that runs `bun --bun <the package's bin script>`. Forge pins the version, not the hash.
- A PyPI or npm install counts as installed only once its `.forge-installed` marker is written, last. An install without its marker is removed and done again.

### 17.4 Downloads

- Only `https://` URLs; a redirect cannot downgrade to plain HTTP. TLS uses the platform's certificate verifier, so a company root CA works. `HTTPS_PROXY`, `ALL_PROXY` and `NO_PROXY` are honoured.
- Timeouts: 15 s to connect, 60 s without receiving data, 15 minutes in all. A download is at most 512 MiB; a text file (`SHA256SUMS`) at most 1 MiB. The user agent is `claude-forge/<version>`.
- A failed download is a note, never a failure of a check: `Could not download <tool> <version> (<url>): <why>. The checks that use it are skipped until it is downloaded; `forge apply` tries again.`

### 17.5 When Forge downloads

Downloads happen only where a person waits anyway, or in the background:

| Where | What may be downloaded |
|---|---|
| `forge apply` | uv (and `path/` is filled), Serena and what its check starts (17.8), and the pinned tools the active checks name. It prints `forge: installed <tool> <version>, …` and `forge: note: <note>` for each failure. |
| `forge check`, Stop | a pinned tool a check or the types engine needs and the cache lacks (the builtins they start get `FORGE_TOOL_DOWNLOAD=1`). |
| SessionStart | nothing in the hook process. The detached `forge engine start` it leaves may install ty (§15.3); the launcher may start a background download of the pinned Forge version (§3). |
| every other hook (PreToolUse, PostToolUse, the typecheck, UserPromptSubmit, PermissionRequest, Notification, SessionEnd) | nothing: a tool is looked up with a few `stat` calls and skipped when absent. |

With `FORGE_NO_DOWNLOAD` set to a non-empty value, Forge downloads no tool: `forge apply` notes `Forge's tools were not installed: downloads are turned off (FORGE_NO_DOWNLOAD)`. The launcher downloads no pinned Forge version either (§18.3): it runs the installed one, and the note for the person says `The download did not happen: downloads are turned off.`

When a pinned tool is not in the cache and may not be downloaded, Forge runs an executable of that name on `PATH`, whatever its version; with none, the check that needs it is skipped with a note. Without a cache at all (no `HOME` and no `FORGE_HOME`), ty, ast-grep, zizmor, condor, hadolint and squawk run through the machine's `uvx` (`uvx -q --from <package>==<version> <binary>`).

### 17.6 Forge's uv on the agent's PATH

Claude Code gives SessionStart hooks a file, `CLAUDE_ENV_FILE`, whose `export` lines run before each Bash command of the session. Where the project's `python.pm` fact (§5) is `uv`, `pyproject` (a pyproject.toml that names no tool: the rules text has the agent run its commands with `uv run --isolated`, which writes no lock file, so the project is not switched to uv) or `none` (no packaging yet), the SessionStart hook appends one line:

```sh
export PATH='<cache>/path'"${PATH:+:$PATH}"
```

The directory comes first and no empty entry is added when `PATH` was empty. The line is appended once: a file that already has it is left alone, and a missing file is created. So the agent's `uv` and `uvx` are Forge's pinned ones, even on a machine without uv; the rules text tells the agent to use uv (§13), never where it lives. A failure is a note: `Could not put Forge's uv on the session's PATH (<file>: <why>).`

### 17.7 Notes

Every provisioning outcome the person must know is a note (§10), never text for the agent: `forge apply` prints it, and the hooks record theirs for `forge status`. The kinds are `missing-tool` (no asset for this platform), `environment` (a download or an install failed: no network, a registry error), `setup` (a sha256 mismatch, an install without its executable) and `forge-failure` (the cache cannot be written, an unknown tool).

### 17.8 Serena

Serena (PyPI `serena-agent`, an MCP server over language servers: navigation by symbol and symbol edits) is one of Forge's tools, part of every Forge project's setup. Its tools are under Forge's rules exactly as the built-in tool each stands next to (§8.6, §9.3, §9.4), and every agent of the session has it, Forge's own agents included (§8.13). `crates/forge-core/src/serena.rs` holds what every crate shares: the language table, the files, the launch arguments, and what each Serena tool is.

**Languages.** One table says, for each language Serena serves, when the project has it and who provides its server; both the current setup and Forge's shared servers (below) read it, so giving Forge a server for a language is one entry there.

| Serena server | The project has it when | Server |
|---|---|---|
| `python_ty` | the python pack is active | Forge's pinned ty, through Forge's hub (below) |
| `typescript` | the typescript pack is active | Serena's (`typescript-language-server` and Serena's pinned TypeScript) |
| `terraform` | the terraform pack is active | Serena's |
| `rust`, `go`, `java`, `ruby`, `php`, `csharp`, `swift` | its manifest is at the root (`Cargo.toml`, `go.mod`, `pom.xml` or `build.gradle[.kts]`, `Gemfile`, `composer.json`, `*.csproj` or `*.sln`, `Package.swift`) | Serena's |

A project with none of them gets no Serena; nor does `forge apply --local`, which writes no shared file (Serena's server goes in `.mcp.json`, and Claude Code takes MCP servers from no settings file): it says so.

**What `forge apply` writes** (project level, Forge-owned, recorded in the lock and removed by `forge remove`):

- `.mcp.json`: the server `serena`, run as `sh -c 'command -v claude-forge >/dev/null 2>&1 && exec claude-forge mcp serena; exec uvx --python 3.12 --from serena-agent==<pin> serena start-mcp-server --project-from-cwd <arguments>'`. The entry has `"alwaysLoad": true`: Serena's tools load with the session instead of waiting behind Claude Code's tool search, where agents rarely look for them (Claude Code waits up to 5 s for them at startup). Where Forge is not installed (a teammate's clone), the same pinned Serena starts with uv and the same arguments, from the same `.serena/`: Serena works there too, without Forge's rules, as none of Forge's hooks run there either.
- `.claude/settings.json`: `enabledMcpjsonServers: ["serena"]`, so Claude Code starts it without asking.
- `.serena/project.yml`: the languages and every key Serena 2.1 reads (`ignore_all_files_in_gitignore: true`, `read_only: false`), so Serena never re-saves it; `.serena/.gitignore`: Serena's own (`/cache`, `/project.local.yml`). The agent's notes (`.serena/memories/`) are the project's, versioned with the code.

Then it installs Serena with the other tools (17.3), starts it once on the project and asks each language's server for the symbols of one of the project's files (downloading what Serena runs for that language on the way): `forge: Serena's language servers start (python_ty, …)`. A server that cannot start stops the apply, as a missing tool would: `forge: Serena's code tools cannot start on this project: <language>: <why>`, with the last lines of Serena's own error output when Serena itself did not start. With downloads turned off it is a note and nothing is checked. When git ignores `.serena/` (a `.gitignore`, or the person's global excludes file), apply says so: the project's settings would not reach other clones. A Serena plugin the person enabled (`serena@…` in Claude Code's user settings) is turned off for the project in `.claude/settings.local.json`, and apply says so: the agent would otherwise have two Serenas.

**`claude-forge mcp serena`** (what `.mcp.json` runs): installs the pinned Serena when it is missing and runs `serena start-mcp-server --project <root> --context claude-code --agent-interface tools --enable-web-dashboard false --open-web-dashboard false --enable-gui-log-window false --log-level WARNING` with `SERENA_HOME=<cache>/serena/home`: Serena's machine configuration, the language servers it downloads and its logs, shared by every repository (a person's own `~/.serena` is never read or changed). Before starting it, it sets `ls_specific_settings` in that home's `serena_config.yml` (the rest of the file as Serena keeps it; a file Serena has not written yet gets `projects: []`, the one key it requires): for each language of the project whose server Forge provides, `ls_base_cmd: [<claude-forge>, "lsp", "<language>"]` and `ls_args: []`. Serena takes a server's command only from there: Forge marks no project trusted in Serena's sense, so a project's `.serena/project.yml` can never make Serena run a command (no `activation_command`, no server path), and TypeScript uses Serena's pinned TypeScript, not the project's `node_modules`. It stays between Claude Code and Serena as a line pass-through: when Serena exits during the session it is started again and the MCP handshake replayed, so the session keeps its tools; the calls in flight get an error to make again. Each restart is a note; after 5 within 5 minutes it is not started again that session. The command line refuses to run for the agent (§3) except `mcp` and `lsp`, which Claude Code and Serena start.

**Serena gets used.** Claude Code's agents reach for searches and whole-file reads even with Serena loaded, so Forge's PreToolUse does what Serena's own `remind` hook does (§9.3), for every agent of the session, Forge's own included, in a project where Forge set Serena up: after 3 searches (`grep`, `rg`, `git grep`…), 3 reads of code files (Read, or `cat`, `head`, `sed`… on a file of the project's Serena languages), or 4 of both, with no Serena call in between and no gap over 1000 s, the call is refused once with "Several searches and reads of code in a row: use Serena's code tools to find and read code by symbol (get_symbols_overview, find_symbol with include_body, find_referencing_symbols). You may make this call again if you still need it."; the agent is then left alone for 2 minutes. A Serena call resets the counts. The main agent and each subagent count apart (`<session>/serena/<agent>.json`).

**Serena's context is kept.** `claude-code` tells the agent to prefer Serena's tools on code and leaves out the tools Claude Code already has (`read_file`, `create_text_file`, `list_dir`, `find_file`, `search_for_pattern`, `execute_shell_command`). Forge's rules apply to both kinds of tool alike, so following it loses no coverage.

**Forge's shared servers.** `claude-forge lsp <language>` (what Serena runs for a server Forge provides) is a stdio shim to the repository's hub for that language, `claude-forge lsp <language> --hub`, which it starts when none answers on `<state>/<repo>/lsp-<language>.sock` (one starter at a time; the others wait for it). The hub runs one server (`ty server` for `python_ty`) for every client of the repository, Serena in each session:

- the first client's `initialize` starts the server; later clients get the same answer, and their `initialized`, `shutdown` and `exit` are not passed on;
- each request gets the hub's own id towards the server and the client's back on the answer; `$/cancelRequest` follows the mapping;
- the server's requests are answered by the hub (`workspace/configuration`: none; `workspace/workspaceFolders`: the root; others: null);
- one copy of each open document: a client opening a document already open sends its text (a change when it differs), each change is applied (positions in UTF-16, or UTF-8 when the server chose it) and sent whole, and the document closes when its last client closes it or leaves;
- the server's notifications go to every client;
- a server that exits while clients remain is started again, the handshake and the open documents replayed, the requests in flight answered with an error; after 10 minutes without a client the hub stops the server and itself. Its log is `<state>/<repo>/lsp-<language>.log`.

Forge's type questions (§15) keep their own private ty: they edit documents in memory with another configuration. Per repository, Python runs at most two ty processes, whatever the number of sessions.

### Tests

- 17.1, 17.3, 17.4: `crates/forge-tools/tests/provision.rs` (release `.tar.gz`, `.zip` and raw assets; checksum mismatch; network failures as notes; truncated downloads; concurrent installs; unknown tool and missing platform; uv on `path/` and uv tools; markers; repackaged binaries; npm tools through Bun; `ensure_all`), `crates/forge-tools/src/catalog.rs` (unit: the table is well formed; musl lookup), `crates/forge-home/src/lib.rs` (unit: layout, components cannot escape).
- 17.2: `crates/forge-tools/tests/spec_pins.rs` (the generated table is in sync), `crates/forge-cli/src/provisioning.rs` (unit: adapter pins are the catalog's).
- 17.5: `crates/forge-launcher/src/policy.rs` (unit: only SessionStart and apply download Forge), `crates/forge-launcher/src/lib.rs` (unit: `downloads_off_never_fetches_the_pinned_version`), `crates/forge-tools/tests/network.rs` (real downloads, opt-in with `FORGE_TEST_NETWORK=1`). (no test: `FORGE_NO_DOWNLOAD` and the "no download after an edit" path of the resolver are exercised only through the hook and checker tests that run with downloads off.)
- 17.6: `crates/forge-tools/tests/env_file.rs` (appended once, sourced by `sh`; the file is created), `crates/forge-tools/src/env_file.rs` (unit: quoting).
- 17.8: `crates/forge-core/src/serena.rs` (unit: languages, the project files, the machine configuration, tool kinds), `crates/forge-engine/src/hub.rs` (unit: frames, positions), `crates/forge-cli/tests/apply.rs` (snapshots: the entries and files), `crates/forge-hooks/tests/spec_hooks.rs` (Serena's tools under the hooks: shell and configuration refused, internals, the test-edit ask on symbol and text edits, edits checked like Bash). (no test: the launcher's restart and handshake replay, the hub's sharing, and the apply-time check, which need a real Serena and ty; checked by hand: two sessions sharing one ty, a killed Serena started again.)
