## 7. Generated output

`forge apply` writes four things: the rules files (§7.1), settings entries (§7.2, §7.3), the commit-time check when consented (§7.6), and the lock (§12). Nothing else.

### 7.1 Rules files

- Each active pack whose context renders non-empty (§5.4) produces `.claude/rules/forge-<pack-id>.md`, with `/` in the id replaced by `-`. These are the only text Forge writes that the agent reads.
- A file holds the line `These are this project's defaults. When the task or the user explicitly asks for something different, do what they ask and point out the conflict; never override an explicit instruction with these defaults.`, a blank line, then the rendered items, each on its own lines, in the order the pack lists them.
- A pack that applies only to some workspace packages (§5.3) gets `paths:` frontmatter, so Claude Code loads the file only for those packages' files:

  ```
  ---
  paths:
    - "<dir>/**"
  ---
  ```

- The text depends only on the repository's content: the active packs, the files, the declared dependencies and the facts of §5.4. It never depends on what is installed on the machine, so the committed file is true for every teammate.
- A rules file of a pack that is no longer active, or whose context no longer renders, is deleted by the next `apply`.

### 7.2 Permission and sandbox entries

Appended to the level's settings file, each recorded in the lock:

- `permissions.ask`, always: project level `Edit(.forge/**)`, `Edit(forge.yaml)`, `Edit(forge.lock)`, `Edit(.claude/rules/forge-*.md)`; user level `Edit(~/.claude/forge-user.lock)`.
- `permissions.deny` / `permissions.ask` entries from the active rules' `permission` targets (§6.2).
- At user level, `sandbox.<path>` from `sandbox` targets: an array value appends each item; a scalar is set only when the key is absent. Forge never changes a value the person set.

### 7.3 Hook entries

Each Claude Code event Forge uses gets one matcher group, appended to `hooks.<Event>` of the settings file. Every command is the wrapper of §7.4 for one `forge hook` event.

Project level:

| Claude Code event | Matcher | `forge hook` event | Timeout | Options |
|---|---|---|---|---|
| SessionStart | `startup\|resume\|clear\|compact` | `session-start` | 10 s | |
| UserPromptSubmit | | `user-prompt-submit` | 10 s | |
| PreToolUse | `Bash` | `pre-tool-use` | 180 s | `onFailure: "block"` |
| PreToolUse | `Read\|Edit\|Write\|MultiEdit\|NotebookEdit\|Grep\|Glob` | `pre-tool-use` | 180 s | |
| PostToolUse | `Edit\|Write\|MultiEdit\|NotebookEdit\|Bash` | `post-tool-use` | 60 s | |
| | | `post-tool-use-typecheck` | 300 s | `asyncRewake: true` |
| PostToolUseFailure | `Bash` | `post-tool-use-failure` | 60 s | |
| | | `post-tool-use-typecheck` | 300 s | `asyncRewake: true` |
| Stop | | `stop` | 600 s | |
| PermissionRequest | | `permission-request` | 180 s | |
| SessionEnd | | `session-end` | 10 s | |

User level:

| Claude Code event | Matcher | `forge hook` event | Timeout | Options |
|---|---|---|---|---|
| PreToolUse | `Bash` | `user-pre-tool-use` | 180 s | `onFailure: "block"` |
| PermissionRequest | | `user-permission-request` | 180 s | |
| Notification | `permission_prompt\|idle_prompt` | `user-notification` | 5 s | |

- The `user-` events answer from the user-level setup's own rules, so a project with both setups runs each once.
- The 180 s timeouts leave room for a reviewer when nobody is there to answer an ask (§9.5); the usual answer takes milliseconds.
- `onFailure: "block"` on the Bash guard makes a crash or a missing binary block the command (§2.3). The wrapper of §7.4 exits 0 when Forge is not installed, so a teammate without Forge is not blocked.

### 7.4 The hook wrapper

Each entry's `command` is `sh -c '<script>'`, so it means the same whatever shell Claude Code uses. The script calls `claude-forge`:

- for `session-start`: `command -v claude-forge >/dev/null 2>&1 || { printf "<json>\n"; exit 0; }; exec claude-forge hook session-start`, where `<json>` is `{"systemMessage":"Forge is not installed, so this project's checks are off. Install it with: curl -LsSf https://github.com/mde-pach/claude-forge/releases/latest/download/install.sh | sh"}` with its double quotes escaped and the apostrophe written as printf's `\047`;
- for every other event: `command -v claude-forge >/dev/null 2>&1 || exit 0; exec claude-forge hook <event>`.

When Forge is missing, the person sees the SessionStart message once (a synchronous hook's `systemMessage`, §2.4) and the agent sees nothing. The script is POSIX, uses only builtins on the missing path, and contains no single quote or backslash pair, so it also survives a shell such as fish.

### 7.5 Settings files

- Entries are edited into the file's text in place: comments, key order and layout of the rest of the file stay as the person wrote them. A file that is not a JSON(C) object is an error naming it.
- `apply` re-adds a recorded entry that went missing and removes recorded entries the plan no longer wants. It never touches an entry it did not add.
- `--local` writes to `.claude/settings.local.json`, whose changes are not confirmed (it is not shared).

### 7.6 The commit-time check

`apply` offers to check commits with `claude-forge check --changed` (§10), in one of two ways:

- **No hook manager.** Git's own pre-commit hook (`.git/hooks/pre-commit`, or the linked worktree's), created only when absent, with a yes at the prompt (`forge: create <path> running "claude-forge check --changed"?`), `--yes` or `--pre-commit`. Its content:

  ```sh
  #!/bin/sh
  # Generated by forge. Remove with forge remove.
  command -v claude-forge >/dev/null 2>&1 || { echo "forge: claude-forge is not installed, so this commit is not checked" >&2; exit 0; }
  exec claude-forge check --changed
  ```

  It is an owned file, recorded in the lock's `[files]`.
- **A hook manager** (`.husky/`, `lefthook.yml`/`lefthook.yaml`/`.lefthook.yml`, `.pre-commit-config.yaml`, or `core.hooksPath` inside the repository, found in that order). Forge adds one block between `# forge: begin (forge remove deletes this block)` and `# forge: end` that runs `claude-forge check --changed`: a local hook in `.pre-commit-config.yaml`, a `forge-check` command under `pre-commit` in lefthook, `claude-forge check --changed || exit 1` in a shell hook. The diff is shown first. Only `--pre-commit` or a yes at the terminal consents; `--yes` alone does not, because the file is the team's and usually tracked. The block is recorded in the lock's `[[blocks]]` with the sha256 of its lines.
- Nothing is offered when the manager's file already runs the check, when `core.hooksPath` is outside the repository, or without a terminal, `--yes` or `--pre-commit`.
- `apply` and `status` say when commits are not checked: `NOT INSTALLED, so commits are not checked. To install: <hint>`, or `<file> runs the check, but its hook manager is not installed in this clone, so commits are not checked. To activate: <command>`.
- `remove` deletes the hook it created, and takes its block out of the manager's file (deleting the file when Forge created it and nothing else is left). A block whose markers were changed, or with entries added inside it, is left in place with instructions.

### 7.7 User level

`forge apply --user` writes the user-level setup into the home directory:

- `~/.claude/settings.json`: the user-level packs' permission and sandbox entries, the owned `ask` (§7.2) and the user hook entries (§7.3);
- `~/.claude/forge-user.lock`: its lock (§12). The launcher does not read it: a user-level setup pins no Forge version for repositories.

The user level writes no rules files; ast-grep, check and grep targets are project-level only (a warning when a user-level pack has one).

### Tests

- Golden snapshots of everything `apply` writes on the six generator samples, idempotence, no internals in rules files: `crates/forge-cli/tests/apply.rs`.
- Rules files and `paths:` frontmatter, `when`, shared ids: `crates/forge-render/tests/context.rs`, `crates/forge-render/tests/builtin_context.rs`.
- Hook entries: `crates/forge-render/src/hooks.rs::guard_entry` (unit); `crates/forge-hooks/tests/spec_hooks.rs::hook_entries_wire_every_event`.
- User level: `crates/forge-cli/tests/spec_cli.rs::user_level_setup`.
- The wrapper's exact strings and its behaviour in sh, dash and `bash --posix` with Forge missing: `crates/forge-launcher/src/hook.rs` (unit), `crates/forge-launcher/tests/hook_shells.rs`.
- Settings edits: `crates/forge-render/src/settings.rs` (unit).
- The commit-time check's blocks: `crates/forge-render/src/precommit.rs` (unit). Git's hook created with `--yes`, recorded in the lock, removed by `remove`: `crates/forge-cli/tests/apply.rs`.
