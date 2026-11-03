## 2. Stack and platform

### 2.1 Forge itself

- Forge is a Rust workspace (`crates/`, edition 2024). The toolchain is pinned in `rust-toolchain.toml`; the minimum Rust version is the workspace's `rust-version`.
- It builds one binary, installed under two names: `forge` and `claude-forge` (§18). Hook entries and the commit-time check call `claude-forge`, because Foundry also installs a `forge`.
- The built-in packs (`packs/`) are embedded in the binary at build time and parsed at run time (§5.1).
- ast-grep and tree-sitter-bash are linked as libraries. The helpers that used to be Python or JavaScript scripts are Rust. Forge needs no Python, Node or Bun of its own.
- At run time Forge needs `git` (file lists, change sets, snapshots) and a POSIX `sh` (the hook wrappers, §7.3; check commands, §6.2). Every other tool it drives is the project's own or one Forge provisions in its cache (§17).

### 2.2 Platforms

- macOS and Linux, on x86_64 and aarch64. Linux release binaries are static musl builds, so they run on glibc and musl systems alike.
- Paths follow the XDG rules on both systems (§4.3).
- Other platforms build from source but get no downloads: the launcher, the tool provisioner and the Forge release fetcher find no asset for them.

### 2.3 Claude Code

- Claude Code ≥ 2.1.295. `forge apply` and `forge init` run `claude --version` and refuse to run when Claude Code is missing (`forge: Claude Code not found; forge needs claude >= 2.1.295`) or older (`forge: Claude Code <v> is too old; forge needs claude >= 2.1.295`).
- Why 2.1.295: the PreToolUse Bash entry sets `onFailure: "block"`, so a guard that crashes or is missing blocks the command instead of letting it through. Claude Code's hooks documentation states that `onFailure` requires v2.1.295 or later. On older versions an exit code other than 0 or 2 is a non-blocking error, so the guard would fail open.
- The lock records the minimum it was written for (`claude_code_min`, §12).

### 2.4 Claude Code behaviour Forge relies on

These are facts of Claude Code's hooks protocol, checked against its documentation (code.claude.com/docs/en/hooks) and, where marked, against a running Claude Code.

- PreToolUse fires for every tool, Read, Grep and Glob included. With `permissionDecision: "deny"`, the `permissionDecisionReason` is shown to Claude. Settings `permissions.deny` entries carry no custom reason, so the internals refusal (§9.3) is a hook decision.
- A synchronous hook's `systemMessage` is shown to the user. An asynchronous hook's goes to Claude, so person-facing messages come only from synchronous hooks (SessionStart, §8.4).
- `CLAUDE_ENV_FILE` is set for SessionStart hooks; the `export` lines written to it run before each Bash command of the session. Forge puts its uv directory on the agent's `PATH` this way (§17.6).
- Hook entries accept `command`, `timeout`, `onFailure` (`continue` | `block`) and `asyncRewake`. An `asyncRewake` hook runs in the background; exit 2 wakes Claude with its stderr.
- Notification matchers include `permission_prompt` and `idle_prompt`; the `terminalSequence` output accepts OSC 0/1/2/9/99/777 and is interactive only.
- Permission rules: `Bash(x:*)` is valid; `~/` and `**` work in Read and Edit rules; absolute paths use `//`. Sandbox keys: `sandbox.enabled`, `sandbox.network.allowedDomains`.
- Who can answer a question (verified on Claude Code 2.1.308, not documented): `claude -p` sets `CLAUDE_CODE_ENTRYPOINT=sdk-cli` and the interactive CLI `cli`. PermissionRequest fires in `-p` in default, auto and bypassPermissions modes but not in dontAsk. A PreToolUse `ask` in `-p` is denied without a PermissionRequest, so the guard reviews it itself (§9.5). A PreToolUse `allow` does not override a settings `deny`.

### Tests

- The Claude Code gate: `crates/forge-cli/tests/apply.rs::an_old_claude_code_is_refused`.
- Hook entries' fields: `crates/forge-render/src/hooks.rs` (unit); `crates/forge-launcher/tests/hook_shells.rs`.
- Platform keys and musl lookup: `crates/forge-tools/src/platform.rs`, `crates/forge-tools/src/catalog.rs` (unit).
- Claude Code itself loading the entries and acting on their output: the real-Claude-Code end-to-end job (§14.6).
