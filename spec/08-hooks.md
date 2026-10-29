## 8. Hooks

Claude Code runs Forge through the hook entries `forge apply` writes (§7). Each entry runs `claude-forge hook <event>` through the missing-binary wrapper (§3), one process per call. The process reads the hook input as JSON on stdin and answers on stdout, per the Claude Code hooks protocol. The guard (PreToolUse Bash) is §9.

### 8.1 Who reads what

- **The agent** reads only findings (§10) and refusal or ask reasons: `additionalContext`, `permissionDecisionReason`, a Stop `reason`, the typecheck's stderr. This text names no rule id and none of Forge's internals: its files, commands, variables or state (`forge_core::internals`). A finding reads `path:line message`, with `Advice: ` before the message of an advice finding.
- **The person** reads `systemMessage`, which only the synchronous SessionStart hook prints (an asynchronous hook's `systemMessage` goes to Claude), and the notes: everything else meant for the person (a missing tool, a rule not enforced, a failure of Forge itself, the environment) is a Note. Hooks never print notes. They record them in the repository's state directory, and `forge status` prints them and clears them (§3).
- **Forge's own failures never block.** A hook that cannot find its project, its lock, its rules, the engine or git says nothing and exits 0. A panic in a hook is caught and answered with silence. A checker that fails or crashes is recorded as a `ForgeFailure` note ("Forge: the checks after an edit stopped unexpectedly.", "Forge: the Stop check stopped unexpectedly."). A Stop check with notes and no error finding passes.
- Input Claude Code leaves out, adds or garbles never fails a hook: every input field is optional, and input that is not JSON is read as an empty input. An event `forge hook` does not know is answered with silence.
- Every hook exits 0, except the typecheck, which exits 2 to wake Claude (§8.7).

### 8.2 Entries

`forge apply` writes these entries (`forge_render::hooks`). The 180 s timeouts leave room for a reviewer (§9.5); the usual answer takes milliseconds.

Project level (`.claude/settings.json`, or `.claude/settings.local.json` with `--local`):

| Claude Code event | Matcher | `forge hook` event | Timeout | Options |
|---|---|---|---|---|
| SessionStart | `startup\|resume\|clear\|compact` | `session-start` | 10 s | |
| UserPromptSubmit | | `user-prompt-submit` | 10 s | |
| PreToolUse | `Bash` | `pre-tool-use` | 180 s | `onFailure: "block"` |
| PreToolUse | `Read\|Edit\|Write\|Grep\|Glob\|NotebookEdit` | `pre-tool-use` | 180 s | |
| PostToolUse | `Edit\|Write\|Bash` | `post-tool-use` | 60 s | |
| PostToolUse | `Edit\|Write\|Bash` | `post-tool-use-typecheck` | 300 s | `asyncRewake: true` |
| PostToolUseFailure | `Bash` | `post-tool-use-failure` | 60 s | |
| PostToolUseFailure | `Bash` | `post-tool-use-typecheck` | 300 s | `asyncRewake: true` |
| Stop | | `stop` | 600 s | |
| PermissionRequest | | `permission-request` | 180 s | |
| SessionEnd | | `session-end` | 10 s | |

User level (`~/.claude/settings.json`):

| Claude Code event | Matcher | `forge hook` event | Timeout | Options |
|---|---|---|---|---|
| PreToolUse | `Bash` | `user-pre-tool-use` | 180 s | `onFailure: "block"` |
| PermissionRequest | | `user-permission-request` | 180 s | |
| Notification | `permission_prompt\|idle_prompt` | `user-notification` | 5 s | |

- The `user-` events run the user-level setup's rules, not the project's, so a project that also has a user-level setup runs each set once.
- `onFailure: "block"` makes the guard fail closed: a guard process that crashes blocks the command. This needs Claude Code ≥ 2.1.295 (§2).
- The PreToolUse entry for Bash runs the guard and the Bash snapshot in one process. The other PreToolUse entry runs the internals refusal and the test-edit ask (§9.3, §9.4).

### 8.3 The project and its state

- A project-level hook works on the repository of the input's `cwd` (else `CLAUDE_PROJECT_DIR`, else the process's directory). Forge walks up from there to the nearest directory holding `forge.lock`, else the nearest holding `forge.yaml`, and stops at the git root. Outside such a repository every project-level hook is silent.
- The hooks read the active rules through a cache in the state directory (`hook-cache.json`): the guard rules, the languages with a type check or typed rules, the active rule ids, and the `python.pm` and `js.pm` facts (§5). The cache is rebuilt when the lock's size or mtime changes and its `rule_set` differs, or when another Forge build runs. The facts are read again when a packaging file at the root (`uv.lock`, `poetry.lock`, `pdm.lock`, `Pipfile`, `Pipfile.lock`, `pyproject.toml`, `setup.py`, `setup.cfg`, `requirements*`, `package.json`, the JS lock files, `pnpm-workspace.yaml`) changes name, size or mtime. Without a lock the project has no active rules and the hooks that need them say nothing.
- State lives in the state root (§4), never in the repository, so the agent's Bash cannot forge a verdict, an approval or a Stop result. Directories are created readable by the user only (mode 0700); files are written through a temporary file and a rename.

```text
<state>/<repo-id>/hook-cache.json             the hooks' view of the active rules
<state>/<repo-id>/notes/hooks.jsonl           notes for the person (the 50 newest, each text once)
<state>/<repo-id>/notes/launcher-<kind>.txt   the launcher's notes (one per kind)
<state>/<repo-id>/reviews.log                 reviewer verdicts
<state>/<repo-id>/timings.<phase>.json        Forge's and the tools' time in the last post-edit / stop check
<state>/<repo-id>/sessions/<session>/turn/    the turn snapshot
<state>/<repo-id>/sessions/<session>/bash/    Bash snapshots
<state>/<repo-id>/sessions/<session>/blocks   Stop blocks this turn
<state>/<repo-id>/sessions/<session>/prompt   the user's prompt for the turn
<state>/<repo-id>/sessions/<session>/review-cache, rewake
<state>/user/                                 the user-level hooks' state outside a project
```

`<repo-id>` is the first 16 hex digits of the sha256 of the repository root's physical path. `<session>` is the input's `session_id` with characters outside `[A-Za-z0-9_-]` replaced by `_` (`default` when absent).

- Git runs with `GIT_OPTIONAL_LOCKS=0`, `core.quotePath=false` and `maintenance.auto=false`, each call capped at 5 s. On macOS, `/usr/bin/git` (a shim that looks up the developer tools on every call) is replaced by the git it runs, found once and kept in `<state>/facts/git-exe`. `FORGE_GIT` names the git to run.

### 8.4 SessionStart (`session-start`)

1. **Context for the agent.** At most 20 lines, as `additionalContext`:

   ```text
   Git branch <branch>, <n> changed files:
     <path>                    (the first 10)
     (<k> more)
   Last commits:
     <%h %s, at most 72 characters>   (the last 5)
   ```

   The branch reads `(detached at <7 hex>)` or `(no commits yet)` when there is none; `1 changed file`; a `.` instead of `:` when nothing changed. The context holds repository facts only: no pack, tool or Forge state.
2. **uv on the agent's PATH.** Where the project's `python.pm` fact is `uv` or `none` (no packaging), the hook appends an `export PATH=<cache>/path:$PATH` line to `CLAUDE_ENV_FILE`, so Forge's uv and uvx are on the PATH of the agent's Bash commands (§17). A failure to write it is a note.
3. **The types engine.** When an active rule needs the types engine (§15), the hook registers the session with the running engine, or starts `forge engine start` detached (never twice: a start in progress holds `<socket>.lock`). `FORGE_ENGINE=off` never starts it. A failure to start is a `ForgeFailure` note.
4. **Cleanup.** This session's Bash snapshots older than a day, and other sessions' directories untouched for three days, are removed.
5. **For the person.** When the launcher ran another version than the one `forge.lock` pins because that one is not downloaded yet (§3), the hook prints the launcher's note as `systemMessage`, in and outside a project: "This project pins Forge <locked>, which is not downloaded yet, so Forge <running> ran instead. …". Missing tools and tool versions are never printed here: `forge apply` and `forge status` print them.

### 8.5 UserPromptSubmit (`user-prompt-submit`)

Prints nothing. It:

- writes the **turn snapshot**: `head` (the commit), `tree` (a hash of HEAD and every uncommitted path's blob), `dirty` (`<path>\t<blob>`, `-` for a deleted file) and `files/<path>` (a copy of each dirty file up to 1 MiB). The paths come from one `git status --porcelain=v2 --untracked-files=all`; ignored files, `.claude/worktrees/` and `.git/` are left out. The snapshot is the base the post-edit and Stop checks compare with (§10);
- removes the Stop block counter;
- saves the prompt (its first 8000 characters) for the test-edit reviewer (§9.5);
- tells a running types engine which files changed since the last prompt (the dirty paths, and the files changed between the commit it last saw and HEAD).

### 8.6 PreToolUse and PostToolUse (`pre-tool-use`, `post-tool-use`, `post-tool-use-failure`)

**Bash snapshot (PreToolUse Bash).** After the guard lets a command through or asks (a denied command never runs), the hook snapshots the working tree as a git tree: `git add -A` and `git write-tree` with `GIT_INDEX_FILE` set to a copy of the repository's index, `GIT_OBJECT_DIRECTORY` set to `<session>/bash/objects` and the repository's objects as `GIT_ALTERNATE_OBJECT_DIRECTORIES`. The user's index and object store are never written. `.claude/worktrees/` is excluded. The tree id goes to `<session>/bash/<tool_use_id>.tree`.

- A snapshot that takes longer than `FORGE_SNAPSHOT_TIMEOUT` seconds (default 3) is recorded as `skipped`, and the person gets an `Environment` note once: "The working tree could not be snapshotted within <n>s, so the files Bash commands change are not checked after each command; the Stop check still covers them. Set FORGE_SNAPSHOT_TIMEOUT to allow more time." The command's changes are then left to Stop.
- Each snapshot drops the `.tree` and `.files` entries beyond the 50 newest (a command denied after the snapshot, or never finished, leaves its entry), and the object directory once no snapshot is pending.

**The files a tool call changed (PostToolUse, PostToolUseFailure).**

- Edit, Write, MultiEdit, NotebookEdit: the edited file (`file_path`, or `notebook_path`), when it is inside the repository and still a file.
- Bash: a second snapshot, compared with the first (`git diff-tree -r --no-renames --name-status`): the added and modified files that still exist. The list is written to `<tool_use_id>.files` for the typecheck. A `skipped` snapshot gives no files. PostToolUseFailure runs the same for a Bash command that failed or was interrupted, which may have written files before it stopped.
- Files under `.forge/`, `.claude/`, `.git/`, `node_modules/` and `.venv/` (at any depth) are never checked. At most 20 files are checked per call; Stop covers the rest.

**The edit-time checks.** The hook runs the checker on those files against the turn snapshot (before the first prompt, against HEAD), with a 45 s budget (§10). When there are findings it answers:

```json
{"hookSpecificOutput": {"hookEventName": "PostToolUse", "additionalContext": "<findings>"}}
```

with `PostToolUseFailure` as the event name after a failed command, and at most 20 lines (`(<n> more lines)` last when there were more). No findings: nothing is printed. A clean edit, a check skipped for a missing tool and a Forge failure all print nothing.

**Test edits.** A PostToolUse of an Edit or Write records the file in `<turn>/touched` and moves the tests the PreToolUse asked about to `<turn>/tests-approved`: the call ran, so they were approved (§9.4).

Not covered after each command: writes by a `run_in_background` command after it returns, and anything outside the repository. Stop still sees them.

### 8.7 The typecheck (`post-tool-use-typecheck`)

The entry is `asyncRewake`: Claude Code runs it in the background, and exit 2 wakes Claude with its stderr.

- It runs for the edited file (Bash: the files in `<tool_use_id>.files`, waiting up to 60 s for PostToolUse to write the list), for the languages whose type check is active: `.py`, `.pyi` (Python) and `.ts`, `.tsx`, `.mts`, `.cts` (TypeScript).
- It asks the running types engine (§15) for the diagnostics the edit added, against the file's base copy (the turn snapshot's copy, else HEAD's). It never starts the engine and never runs a type checker itself: with no engine running, or an engine still loading, it says nothing, and Stop runs the type check.
- New errors: exit 2 with `This edit added type errors:` and at most 20 diagnostic lines (`(<n> more)` after them) on stderr. An engine error is a `ForgeFailure` note.
- Claude Code runs an `asyncRewake` hook in the background only in an interactive session or with streaming input. Under `claude -p "<prompt>"` every edit would wait for it, so when `CLAUDE_CODE_ENTRYPOINT` is `sdk-cli` and no ancestor process (up to 12) runs with `--input-format stream-json` or `--sdk-url`, the hook exits 0 at once and Stop runs the type check. The answer is kept per session (`<session>/rewake`). `FORGE_EDIT_TYPECHECK=always` runs it anyway; `never` never runs it.

### 8.8 Stop (`stop`)

The Stop check runs the turn's checks (§10) and blocks with the findings: `{"decision": "block", "reason": "<findings>"}`.

1. A Stop that is not a continuation of a blocked one (`stop_hook_active` false) starts a new block count.
2. Nothing changed since the prompt (the tree hash equals the turn snapshot's): no check. Work the user left uncommitted before the turn never blocks Claude.
3. A tree that already passed passes again without a check (`<turn>/pass`; without a turn snapshot, `<session>/last-pass`).
4. A tree that already failed is not checked again: the block repeats the stored reason after "Nothing changed in the working tree since this check failed, so it was not run again (it runs again once a file changes):".
5. Otherwise the checker runs on the turn's change (against the turn snapshot; without one, on every uncommitted change), with a 300 s budget.
   - Error findings: block. The reason holds errors first, then advice, at most 40 lines and 6000 characters. The tree as the checks left it (formatters may have run) is recorded as failed.
   - Advice only: block once; the same tree then passes.
   - Only tests that also fail at the start of the turn (§10): block once. Stopping again on the same tree is let through, with the note "Some tests could not be verified: they fail with and without Claude's change. Run them once the environment is set up."
   - No error finding (a pass, a skipped check, a Forge failure): pass, and the tree is recorded as passed.
6. After 3 blocks in one turn the stop is let through, with the note "The Stop check failed 3 times in one turn, so Claude was let stop with the failures left; run `forge check` to see them." A new prompt resets the count.

A commit made during the turn is part of the turn: the snapshot's `head` is the base.

### 8.9 PermissionRequest, SessionEnd, Notification

- **PermissionRequest** (`permission-request`, `user-permission-request`): the reviewer of §9.5.
- **SessionEnd** (`session-end`): unregisters the session from the types engine (the last session to end stops it, §15) and removes the session's Bash snapshots. Prints nothing.
- **Notification** (`user-notification`): prints `{"terminalSequence": "<seq>"}`. The sequence is OSC 9 (`ESC ]9;<msg> BEL`) when `TERM_PROGRAM` is `iTerm.app` or `WezTerm`, else OSC 777 (`ESC ]777;notify;Claude Code;<msg> BEL`). The message is the input's `message` without control characters, at most 200 characters; an empty one reads "Claude Code needs your attention". Claude Code shows terminal sequences in interactive sessions only.

### 8.10 Environment variables

| Variable | Effect | Default |
|---|---|---|
| `CLAUDE_CODE_ENTRYPOINT` | Set by Claude Code. `sdk-cli`, `sdk-ts`, `sdk-py`, `claude-code-github-action`: nobody can answer an ask (§9.5); `sdk-cli`: the typecheck may wait (§8.7) | |
| `CLAUDE_ENV_FILE` | Set by Claude Code at SessionStart: where the PATH line goes | |
| `CLAUDE_PROJECT_DIR` | The project when the input has no `cwd` | |
| `TERM_PROGRAM` | The notification's sequence | |
| `FORGE_ASK_REVIEW` | `always`: review every ask; `never`: never review; else auto (§9.5) | auto |
| `FORGE_REVIEW_MODEL` | The reviewer's `--model` | Claude Code's |
| `FORGE_REVIEWER` | Set inside a review: no review there | |
| `FORGE_EDIT_TYPECHECK` | `always` / `never` run the edit-time typecheck (§8.7) | auto |
| `FORGE_ENGINE` | `off`: SessionStart never starts the types engine | on |
| `FORGE_SNAPSHOT_TIMEOUT` | Seconds a Bash snapshot may take | 3 |
| `FORGE_GIT` | The git executable | the one on PATH |
| `FORGE_HOME`, `XDG_CACHE_HOME`, `XDG_STATE_HOME` | The cache and state roots (§4) | |

The edit-time and Stop budgets (45 s and 300 s, under their hooks' 60 s and 600 s timeouts) are fixed. A check still running when the budget runs out is stopped, and the person gets a `ForgeFailure` note: it never blocks.

### 8.11 Budgets

Every hook runs on Claude's path, so Forge's own time is budgeted. The budgets are warm (hook cache built, a turn snapshot taken), on the `fastapi-app` generator sample after `forge apply`, on a release build; only the first edit of a session may exceed them. "Forge overhead" is the hook process's wall time less the time the project's tools ran (recorded per phase in `timings.<phase>.json`).

| Hook | Budget | Measured (median / p99) | Asserted by |
|---|---|---|---|
| PreToolUse Bash, guard decision (a deny) | p99 ≤ 10 ms | 4.8 / 6.5 ms | `crates/forge-cli/tests/hook.rs::budgets` |
| PreToolUse Bash, guard + snapshot | p99 ≤ 50 ms | 22.3 / 30.2 ms | `hook.rs::budgets` |
| PreToolUse Edit of a test (test-edit lookup) | p99 ≤ 10 ms | 7.5 / 9.9 ms | `hook.rs::budgets` |
| PreToolUse Read (internals refusal) | p99 ≤ 10 ms | 4.6 / 5.2 ms | `hook.rs::budgets` |
| UserPromptSubmit | p99 ≤ 50 ms | 20.5 / 36.6 ms | `hook.rs::budgets` |
| SessionStart | p99 ≤ 200 ms | 21.6 / 31.5 ms | `hook.rs::budgets` |
| PostToolUse Edit (Python), Forge overhead | median ≤ 30 ms, p99 ≤ 45 ms; total median ≤ 1 s | 24.9 / 46.1 ms; total 550 / 1505 ms | `hook.rs::budgets` |
| Stop, Forge overhead | median ≤ 300 ms, p99 ≤ 450 ms | 180.9 / 288.6 ms (tools 2.6 s median) | `hook.rs::budgets` |
| Edit-time checker overhead (ast-grep rules of the Python packs, one file, debug build) | median < 500 ms | printed | `crates/forge-check/tests/change_set.rs::post_edit_overhead` |
| Types engine, one answer after an edit, warm | median < 300 ms | printed | `crates/forge-engine/tests/engine.rs::python_engine` and the `typescript_*_engine` tests (skip without uv / the TypeScript versions) |
| PermissionRequest, SessionEnd, Notification | none | not measured | (no test: reviewer time dominates; SessionEnd and Notification do no checking) |

Measured on an Apple M2, 2026-10-29, while other builds ran on the machine (not a quiet run; the PostToolUse p99 then went 1 ms over its bound). Measured, not asserted: `hook.rs::budgets` is `#[ignore]` and runs only as `FORGE_BUDGETS=1 cargo test --release -p forge-cli --test hook -- --ignored --nocapture`, on a quiet machine. The other timing tests run with the suite.

### 8.12 Claude Code behaviour this chapter relies on

Verified against the Claude Code hooks documentation and, where noted, on real sessions:

- PreToolUse matches every tool except EndConversation, Read, Grep and Glob included. A `permissionDecision: "deny"` reason is shown to Claude; settings `permissions.deny` has no custom reason, which is why the refusals of §9 are hooks.
- A synchronous hook's `systemMessage` is shown to the user; an asynchronous hook's goes to Claude.
- Hook entries accept `timeout`, `onFailure` (`continue` | `block`, Claude Code ≥ 2.1.295) and `asyncRewake` (exit 2 wakes Claude with stderr). On 2.1.284–2.1.294 a guard crash fails open, because exit codes other than 0 and 2 are non-blocking errors.
- `CLAUDE_ENV_FILE` is given to SessionStart; its `export` lines run before each Bash command.
- Notification matchers include `permission_prompt` and `idle_prompt`; `terminalSequence` accepts OSC 0/1/2/9/99/777 and is interactive-only.
- Claude Code runs an `asyncRewake` hook in the background only in an interactive session or with streaming input (`--input-format stream-json`, `--sdk-url`).
- Headless runs need a trusted workspace for project settings and their hooks to load.

### Tests

- 8.1 Who reads what: `crates/forge-hooks/tests/events.rs` (every test asserts agent output names no internals; `stop_blocks_on_findings_not_on_notes`, `outside_a_project_hooks_are_silent`), `crates/forge-hooks/tests/spec_hooks.rs::a_checker_crash_is_silent_and_noted`, `crates/forge-cli/tests/hook.rs::hooks_on_an_applied_sample` (garbage input), `crates/forge-hooks/src/input.rs (unit)`.
- 8.2 Entries: `crates/forge-hooks/tests/spec_hooks.rs::hook_entries_wire_every_event`, `crates/forge-render/src/hooks.rs (unit)`, `crates/forge-launcher/src/hook.rs (unit)`, `crates/forge-launcher/tests/hook_shells.rs`.
- 8.3 Project and state: `spec_hooks.rs::state_lives_outside_the_repository_readable_by_the_user_only`, `spec_hooks.rs::notes_keep_the_fifty_newest`, `crates/forge-hooks/src/state.rs (unit)`, `crates/forge-hooks/src/rules.rs (unit)`, `crates/forge-hooks/src/proc.rs (unit)`, `crates/forge-home/src/lib.rs (unit)`.
- 8.4 SessionStart: `events.rs::session_start_gives_repository_context_only`, `events.rs::session_start_starts_the_engine_for_typed_rules`, `spec_hooks.rs::session_start_shows_a_missing_pinned_version_to_the_person_only`, `spec_hooks.rs::session_start_puts_uv_on_path_only_for_uv_or_unpackaged_projects`, `crates/forge-tools/tests/env_file.rs`.
- 8.5 UserPromptSubmit: `events.rs::prompt_snapshots_the_turn_silently`, `crates/forge-hooks/src/snapshot.rs (unit)`. The engine resync: (no test: needs a running engine; covered only by reading).
- 8.6 PreToolUse / PostToolUse: `events.rs::post_tool_use_reports_findings_on_the_changed_files`, `spec_hooks.rs::a_snapshot_past_its_time_is_skipped_and_left_to_stop`, `spec_hooks.rs::bash_changes_leave_the_users_index_alone_and_skip_tool_directories`, `spec_hooks.rs::bash_snapshots_beyond_the_fifty_newest_are_dropped`.
- 8.7 Typecheck: `events.rs::typecheck_reports_new_errors_through_the_engine`, `spec_hooks.rs::edit_typecheck_never_is_silent`. The stream-json ancestor lookup: (no test: needs a real ancestor process with those arguments).
- 8.8 Stop: `events.rs::stop_blocks_on_findings_not_on_notes`, `events.rs::advice_blocks_once`, `spec_hooks.rs::stop_remembers_a_passing_tree`, `spec_hooks.rs::stop_without_a_turn_snapshot_checks_every_change`, `spec_hooks.rs::a_commit_during_the_turn_is_part_of_it`, `spec_hooks.rs::the_block_reason_is_capped_and_a_new_prompt_resets_the_count`, `spec_hooks.rs::tests_failing_before_the_turn_too_block_once`.
- 8.9 PermissionRequest, SessionEnd, Notification: `events.rs::notification_and_session_end`, `spec_hooks.rs::notifications_use_osc_9_where_the_terminal_reads_it`; reviewer tests under §9.
- 8.10 Environment variables: covered by the tests of the behaviour each one changes.
- 8.11 Budgets: as in the table.
- 8.12 Claude Code behaviour: the real-Claude-Code end-to-end job (§14); not by the unit suite.
