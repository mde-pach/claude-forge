## 8. Hooks

Claude Code runs Forge through the hook entries `forge apply` writes (§7). Each entry runs `claude-forge hook <event>` through the missing-binary wrapper (§3), one process per call. The process reads the hook input as JSON on stdin and answers on stdout, per the Claude Code hooks protocol. The guard (PreToolUse Bash) is §9.

### 8.1 Who reads what

- **The agent** reads findings (§10), refusal or ask reasons, and the capabilities selected for its task (§20): `additionalContext`, `permissionDecisionReason`, a Stop `reason`, the typecheck's stderr. This text names no rule id and none of Forge's internals: its files, commands, variables or state (`forge_core::internals`). A finding reads `path:line message`, with `Advice: ` before the message of an advice finding. A capability's text is its file's own (§20.5).
- **The person** reads `systemMessage`, which only the synchronous SessionStart hook prints (an asynchronous hook's `systemMessage` goes to Claude), and the notes: everything else meant for the person (a missing tool, a rule not enforced, a failure of Forge itself, the environment) is a Note. Hooks never print notes. They record them in the repository's state directory, and `forge status` prints them and clears them (§3).
- **Forge's own failures never block.** A hook that cannot find its project, its lock, its rules, the types engine or git says nothing and exits 0. A panic in a hook is caught and answered with silence. A checker that fails or crashes is recorded as a `ForgeFailure` note ("Forge: the checks after an edit stopped unexpectedly.", "Forge: the Stop check stopped unexpectedly."). A Stop check with notes and no error finding passes.
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
| PreToolUse | `Read\|Edit\|Write\|MultiEdit\|NotebookEdit\|Grep\|Glob` | `pre-tool-use` | 180 s | |
| PostToolUse | `Edit\|Write\|MultiEdit\|NotebookEdit\|Bash` | `post-tool-use` | 60 s | |
| PostToolUse | `Edit\|Write\|MultiEdit\|NotebookEdit\|Bash` | `post-tool-use-typecheck` | 300 s | `asyncRewake: true` |
| PostToolUseFailure | `Bash` | `post-tool-use-failure` | 60 s | |
| PostToolUseFailure | `Bash` | `post-tool-use-typecheck` | 300 s | `asyncRewake: true` |
| PostToolBatch | | `post-tool-batch` | 30 s | |
| Stop | | `stop` | 600 s | |
| PermissionRequest | | `permission-request` | 180 s | |
| SessionEnd | | `session-end` | 10 s | |
| PreToolUse | `*` | `agents` | 30 s | |
| PostToolUse | `Agent\|Task` | `agents` | 30 s | |
| SubagentStart | `quality-reviewer` | `agents` | 30 s | |
| SubagentStop | `quality-reviewer` | `agents` | 30 s | |

User level (`~/.claude/settings.json`):

| Claude Code event | Matcher | `forge hook` event | Timeout | Options |
|---|---|---|---|---|
| PreToolUse | `Bash` | `user-pre-tool-use` | 180 s | `onFailure: "block"` |
| PreToolUse | `Read\|Edit\|Write\|MultiEdit\|NotebookEdit\|Grep\|Glob` | `user-pre-tool-use` | 180 s | |
| PermissionRequest | | `user-permission-request` | 180 s | |
| Notification | `permission_prompt\|idle_prompt` | `user-notification` | 5 s | |
| PreToolUse | `Agent\|Task\|Read\|Grep\|Glob` | `user-agents` | 30 s | |
| PostToolUse | `Agent\|Task` | `user-agents` | 30 s | |
| SubagentStart | `quality-reviewer` | `user-agents` | 30 s | |
| SubagentStop | `quality-reviewer` | `user-agents` | 30 s | |

- The `agents` and `user-agents` entries run Forge's reviewer (§8.13). The PreToolUse entry matches every tool: the reviewer's own calls make no edits, and none past its time; any other call gets nothing after one state lookup. `user-agents` handles the reviewer when the user-level setup exists, and the project's `agents` then defers to it.
- UserPromptSubmit has 10 s: it may make one request to the capability engine, which it waits for 1.5 s at most (§20.3).
- The `user-` events run the user-level setup's rules, not the project's, so a project that also has a user-level setup runs each set once.
- `onFailure: "block"` makes the guard fail closed: a guard process that crashes blocks the command. This needs Claude Code ≥ 2.1.295 (§2).
- The PreToolUse entry for Bash runs the guard and the Bash snapshot in one process. The other PreToolUse entry runs the internals refusal and the test-edit ask (§9.3, §9.4).

### 8.3 The project and its state

- A project-level hook works on the repository of the input's `cwd` (else `CLAUDE_PROJECT_DIR`, else the process's directory). Forge walks up from there to the nearest directory holding `forge.lock`, else the nearest holding `forge.yaml`, and stops at the git root. Outside such a repository every project-level hook is silent.
- The hooks read the active rules through a cache in the state directory (`hook-cache.json`): the guard rules, the languages with a type check or typed rules, the active rule ids, the rules files the lock records, and the `python.pm` and `js.pm` facts (§5). The active rules follow the repository as it is now, as the checks' do (§10.1): the rules a plan of the repository selects (its packs, local rules, `forge.yaml`, detection), where the lock decides for the packs it records (a rule of such a pack, or a local rule, is active only if the lock records it) and a pack the lock does not record gives the rules the plan selects. A Poetry project moved to uv during the session gets uv's guard rules and loses Poetry's. A plan that fails falls back to the lock's rules. The cache is rebuilt when the lock's size or mtime changes, when a packaging file at the root (`uv.lock`, `uv.toml`, `poetry.lock`, `pdm.lock`, `Pipfile`, `Pipfile.lock`, `pyproject.toml`, `setup.py`, `setup.cfg`, `requirements*`, `package.json`, the JS lock files, `pnpm-workspace.yaml`, `.yarnrc.yml`) changes name, size or mtime, or when another Forge build runs; otherwise a hook reads it with one directory read and no plan. Without a lock the project has no active rules and the hooks that need them say nothing.
- State lives in the state root (§4), never in the repository, so the agent's Bash cannot forge a verdict, an approval or a Stop result. Directories are created readable by the user only (mode 0700); files are written through a temporary file and a rename.

```text
<state>/<repo-id>/hook-cache.json             the hooks' view of the active rules
<state>/<repo-id>/notes/hooks.jsonl           notes for the person (the 50 newest, each text once)
<state>/<repo-id>/notes/launcher-<kind>.txt   the launcher's notes (one per kind)
<state>/<repo-id>/reviews.log                 reviewer verdicts
<state>/<repo-id>/capabilities.log            the capability engine's decisions and the agent's loads (§20.7)
<state>/<repo-id>/timings.<phase>.json        Forge's and the tools' time in the last post-edit / stop check
<state>/<repo-id>/sessions/<session>/turn/    the turn snapshot
<state>/<repo-id>/sessions/<session>/bash/    Bash snapshots
<state>/<repo-id>/sessions/<session>/blocks   Stop blocks this turn
<state>/<repo-id>/sessions/<session>/prompt   the user's prompt for the turn
<state>/<repo-id>/sessions/<session>/review-cache, rewake
<state>/<repo-id>/sessions/<session>/context.json the packs' context already given this session (§8.6)
<state>/<repo-id>/sessions/<session>/capabilities/given the capabilities given this session (§20.5)
<state>/user/                                 the user-level hooks' state outside a project
<state>/user/capabilities/                    this machine's consent to the capability engine and its key (§20.3)
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

   The branch reads `(detached at <7 hex>)` or `(no commits yet)` when there is none; `1 changed file`; a `.` instead of `:` when nothing changed. The context holds repository facts only: no pack, tool or Forge state. After a compaction (`source` `compact`), the current turn's capabilities follow, after a blank line, as the agent read them (§20.5).
2. **uv on the agent's PATH.** Where the project's `python.pm` fact is `uv`, `pyproject` (packaging that names no tool) or `none` (no packaging), the hook appends an `export PATH=<cache>/path:$PATH` line to `CLAUDE_ENV_FILE`, so Forge's uv and uvx are on the PATH of the agent's Bash commands (§17). A failure to write it is a note.
3. **The types engine.** When an active rule needs the types engine (§15), the hook registers the session with the running engine, or starts `forge engine start` detached (never twice: a start in progress holds `<socket>.lock`). `FORGE_ENGINE=off` never starts it. A failure to start is a `ForgeFailure` note.
4. **Cleanup.** This session's Bash snapshots older than a day, and other sessions' directories untouched for three days, are removed.
5. **For the person.** When the launcher ran another version than the one `forge.lock` pins because that one is not downloaded yet (§3), the hook prints the launcher's note as `systemMessage`, in and outside a project: "This project pins Forge <locked>, which is not downloaded yet, so Forge <running> ran instead. …". Missing tools and tool versions are never printed here: `forge apply` and `forge status` print them.

### 8.5 UserPromptSubmit (`user-prompt-submit`)

Gives the capabilities the capability engine selects for the prompt (§20.3, §20.5), and prints nothing else.

Claude Code also delivers its own messages as prompts: a subagent's report (starting `<agent-message`) and the end of a background task (starting `<task-notification>`). Neither is the person's request nor a new turn: the hook does nothing for them, and asks the capability engine nothing.

A turn begins with a prompt and ends when Claude stops (a Stop that is let through marks the turn snapshot `ended`). A prompt that arrives before Claude stopped (one the person queued while it worked, or one sent after an interrupt, which runs no Stop) continues the turn: the work since the turn began is still unchecked, so the snapshot and the Stop count stay, and the prompt is added to the saved request. A prompt after Claude stopped starts a new turn. It:

- writes the **turn snapshot**: `head` (the commit), `tree` (a hash of HEAD and every uncommitted path's blob), `dirty` (`<path>\t<blob>`, `-` for a deleted file) and `files/<path>` (a copy of each dirty file up to 1 MiB). The paths come from one `git status --porcelain=v2 --untracked-files=all`; ignored files, `.claude/worktrees/` and `.git/` are left out. The snapshot is the base the post-edit and Stop checks compare with (§10);
- removes the Stop block counter;
- saves the prompt (its first 8000 characters) for the test-edit reviewer (§9.5);
- tells a running types engine which files changed since the last prompt (the dirty paths, and the files changed between the commit it last saw and HEAD).

Then, on every prompt (one that continues the turn too), it gives the capabilities of §20: the active, available ones not given yet this session go to the capability engine in one bounded request, and the selected ones are the hook's `additionalContext` (§20.5). With none to offer, no request is made.


### 8.6 PreToolUse and PostToolUse (`pre-tool-use`, `post-tool-use`, `post-tool-use-failure`, `post-tool-batch`)

**Bash snapshot (PreToolUse Bash).** After the guard lets a command through or asks (a denied command never runs), the hook snapshots the working tree as a git tree: `git add -A` and `git write-tree` with `GIT_INDEX_FILE` set to a copy of the repository's index, `GIT_OBJECT_DIRECTORY` set to `<session>/bash/objects` and the repository's objects as `GIT_ALTERNATE_OBJECT_DIRECTORIES`. The user's index and object store are never written. `.claude/worktrees/` is excluded. The tree id goes to `<session>/bash/<tool_use_id>.tree`.

- A snapshot that takes longer than `FORGE_SNAPSHOT_TIMEOUT` seconds (default 3) is recorded as `skipped`, and the person gets an `Environment` note once: "The working tree could not be snapshotted within <n>s, so the files Bash commands change are not checked after each command; the Stop check still covers them. Set FORGE_SNAPSHOT_TIMEOUT to allow more time." The command's changes are then left to Stop.
- Each snapshot drops the `.tree` and `.files` entries beyond the 50 newest (a command denied after the snapshot, or never finished, leaves its entry), and the object directory once no snapshot is pending.

**The files a tool call changed (PostToolUse, PostToolUseFailure).**

- Edit, Write, MultiEdit, NotebookEdit: the edited file (`file_path`, or `notebook_path`), when it is inside the repository and still a file.
- Serena's edits (§17.8): `replace_content`, `replace_symbol_body`, `insert_before_symbol`, `insert_after_symbol`, `rename_symbol`, `replace_in_files`, `safe_delete_symbol`, and any Serena tool Forge does not know: as a Bash command, from a snapshot the PreToolUse takes before them, so a rename across files is checked file by file. Serena's navigation and notes change nothing to check.
- Bash: a second snapshot, compared with the first (`git diff-tree -r --no-renames --name-status`): the added and modified files that still exist. The list is written to `<tool_use_id>.files` for the typecheck. A `skipped` snapshot gives no files. PostToolUseFailure runs the same for a Bash command that failed or was interrupted, which may have written files before it stopped.
- Files under `.forge/`, `.claude/`, `.git/`, `node_modules/` and `.venv/` (at any depth) are never checked. At most 20 files are checked per call; Stop covers the rest.

**The edit-time checks.** The hook runs the checker on those files against the turn snapshot (before the first prompt, against HEAD), with a 45 s budget (§10). Formatting and safe fixes are written at once, where Claude Code takes the file's new content into account for the agent's next edit.

**Given when the batch ends (`post-tool-batch`).** The agent can make several calls in one answer; Claude Code runs them one after the other, starting the first before the answer is complete, and hands all their results back when the batch ends. A check after one edit sees the file before the batch's later edits (a name used before the edit that adds its import: "Undefined name"; an import added before its use: "imported but unused"), so its findings may be gone by the time the agent reads them. So the hook does not answer with them: it holds them in the session's state under the call's `tool_use_id` (`held/<id>.json`: the files it checked and its findings). The PostToolBatch hook, which gets the batch's calls in the order they ran (`tool_calls`, each with its `tool_use_id`), takes the held checks of those calls and gives, for each file, the findings of the last check that looked at it or found something in it:

```json
{"hookSpecificOutput": {"hookEventName": "PostToolBatch", "additionalContext": "<findings>"}}
```

at most 20 lines (`(<n> more lines)` last when there were more). No findings: nothing is printed. A clean edit, a check skipped for a missing tool and a Forge failure all print nothing. A call without a `tool_use_id`, or whose check cannot be held, answers at once, as PostToolUse (`PostToolUseFailure` after a failed command). A held check whose batch hook never ran is dropped after an hour. Measured on the benchmark's runs before this: 15 of the 24 lint findings given after an edit were already gone when the agent read them, and an agent wrote down not to trust that message.

**Context of packs that became active.** The rules files (§7) are written by `forge apply`, but the checker selects the packs as the repository is now (§10.1): packaging files created during the session (`uv init --bare` and `uv add` in a plain-scripts project) activate packs whose context the agent never read. When the packaging files or the lock changed since the session last looked, PostToolUse compares the rules files the active packs give now with those the lock records. Each one the lock does not record is given once per session, after the findings in the same `additionalContext`, as `This project now uses <pack>:` (the last segment of the pack id) and the file's items, true under their `when`, without its introduction.

**Context that no longer holds.** A rules file the lock records holds the items whose `when` held at apply, as the repository's copy shows (what the agent was given). When an item of that copy no longer holds now (a plain-scripts project's "This project has no Python packaging file: …" once it has a `requirements.txt`), the agent is told `This no longer applies: <item>`; when the file's pack is no longer active, `This project no longer uses <pack>.`. Each is given once per session, before the context of packs that became active. A Stop that blocks puts all of it before its findings. Hooks never rewrite the repository's files: the next `forge apply` writes the rules files.

**Test edits.** A PostToolUse of an Edit or Write (or of a Serena edit of one file) records the file in `<turn>/touched` and moves the tests the PreToolUse asked about to `<turn>/tests-approved`: the call ran, so they were approved (§9.4).

**Files outside the repository.** The rules follow the agent, not the repository: a file the agent writes outside it (a script or a scratch test in `/tmp`) is checked with the project's rules. The files are an Edit's, Write's, MultiEdit's or NotebookEdit's outside the repository, and a Bash command's redirection targets (`>`, `>>`, `&>`: `cat > /tmp/probe.py <<EOF`) whose path is known before the command runs. Left out: files under Claude Code's configuration directory (its memory, plans) and Forge's own directories, the excluded directories above, anything that is not a regular file, and files over 1 MiB (an output, not code). The checker works on a repository, so they are checked in the session's mirror (`<session>/outside`): a git repository whose top-level entries link to the project's (but `.git`), committed once, with a copy of each file at `_outside/<absolute path>`. There the copies are new files and the project's configuration applies; findings and notes are given back under the real path, and what a fixer or a formatter changed in a copy is written back to the file, as in the repository. A new turn drops the last turn's copies.

Not covered after each command: writes by a `run_in_background` command after it returns, and files a command writes outside the repository by other means than a redirection (a program's own output files). Stop still sees the repository's.

### 8.7 The typecheck (`post-tool-use-typecheck`)

The entry is `asyncRewake`: Claude Code runs it in the background, and exit 2 wakes Claude with its stderr.

- It runs for the edited file (Bash: the files in `<tool_use_id>.files`, waiting up to 60 s for PostToolUse to write the list), for the languages whose type check is active: `.py`, `.pyi` (Python) and `.ts`, `.tsx`, `.mts`, `.cts` (TypeScript).
- It asks the running types engine (§15) for the errors of the edited files, old or new (§10.4). It never starts the engine and never runs a type checker itself: with no engine running, or an engine still loading, it says nothing, and Stop runs the type check.
- An unresolved import of a declared dependency is the environment's, as at Stop (§10.4): a note, never in the text.
- New errors: exit 2 with `The edited files have type errors:` and at most 20 diagnostic lines (`(<n> more)` after them) on stderr. A types engine error is a `ForgeFailure` note.
- **The uv lock.** When `python/uv/lock-sync` is active, the edit (or the Bash command) changed `pyproject.toml` or `uv.toml`, and the project has a `uv.lock`, it runs `uv lock --check` and, on a stale lock, `uv lock`: the lock is derived, so Forge keeps it in sync rather than report it (§10.5). A relock adds `uv.lock was updated to match pyproject.toml.` to the stderr (exit 2). A lock the project does not have is never created here (setup, `forge apply`), and a `pyproject.toml` that does not resolve is left to Stop, which reports uv's error. Nor does it relock when `pyproject.toml` declares a dependency name its copy at the start of the turn (the turn snapshot's, else HEAD's) does not: at Stop the lock is updated only after the new-dependency checks (§10.5). The JavaScript locks are not relocked here (§10.5).
- Claude Code runs an `asyncRewake` hook in the background only in an interactive session or with streaming input. Under `claude -p "<prompt>"` every edit would wait for it, so when `CLAUDE_CODE_ENTRYPOINT` is `sdk-cli` and no ancestor process (up to 12) runs with `--input-format stream-json` or `--sdk-url`, the hook exits 0 at once and Stop runs the type check. The answer is kept per session (`<session>/rewake`). `FORGE_EDIT_TYPECHECK=always` runs it anyway; `never` never runs it.

### 8.8 Stop (`stop`)

The Stop check runs the turn's checks (§10) and blocks with the findings: `{"decision": "block", "reason": "<findings>"}`.

1. A Stop that is not a continuation of a blocked one (`stop_hook_active` false) starts a new block count.
2. Nothing changed since the prompt (the tree hash equals the turn snapshot's): no check. Work the user left uncommitted before the turn never blocks Claude.
3. A tree that already passed passes again without a check (`<turn>/pass`; without a turn snapshot, `<session>/last-pass`).
4. A tree that already failed is not checked again. Claude has been shown its findings once and stopped again without changing anything, so it chose to leave them (the user may have asked for that code): the stop is let through, and the person gets the note "Claude stopped with the Stop check's findings left after being shown them once; run `forge check` to see them."
5. Otherwise the checker runs on the turn's change (against the turn snapshot; without one, on every uncommitted change), with a 300 s budget.
   - Error findings: block. The reason holds errors first, then advice, at most 40 lines and 6000 characters. The tree as the checks left it (formatters may have run) is recorded as failed.
   - Advice only: block once; the same tree then passes.
   - No error finding (a pass, a skipped check, a Forge failure): pass, and the tree is recorded as passed.
6. After 3 blocks in one turn the stop is let through, with the note "The Stop check failed 3 times in one turn, so Claude was let stop with the failures left; run `forge check` to see them." A new turn's prompt resets the count.
7. Once the repository's change passed (or nothing changed), the files the turn wrote outside the repository are checked the same way, in the mirror (§8.6), each refreshed from its real file (a deleted one is dropped). Error findings, and advice once, block; the same findings shown once are then let through, and these blocks count toward the turn's 3.

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
| `FORGE_REVIEW_MODEL` | Read by `forge apply`: the `model` of Forge's reviewer (§8.13) | the session's |
| `FORGE_REVIEW_EFFORT` | Read by `forge apply`: the `effort` of Forge's reviewer (`low`, `medium`, `high`, `xhigh`, `max`) | `medium` |
| `FORGE_EDIT_TYPECHECK` | `always` / `never` run the edit-time typecheck (§8.7) | auto |
| `FORGE_ENGINE` | `off`: SessionStart never starts the types engine | on |
| `FORGE_CAPABILITIES` | `off`: no capability is given and the capability engine is never asked (§20) | on |
| `FORGE_CAPABILITIES_URL` | Another base URL for the capability engine's API, for Forge's own tests (§20.3) | `https://api.typesafe.ai` |
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
| UserPromptSubmit with capability selection (the digest, one request to a fake engine on localhost, the rendering), whole hook | p99 ≤ 80 ms | not measured yet | `hook.rs::budgets` |
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

### 8.13 Forge's own agent (`agents`, `user-agents`)

Forge never starts a `claude -p` process. What it asks a model, it asks its reviewer, `quality-reviewer`, a native Claude Code subagent of the session, so it appears in the session with its own transcript and gets its environment. It runs one review of an ask (§9.5). It has every tool of the session, Serena's included (§17.8): its definition names no `tools`. What it may not do, the hooks refuse (below). Its name is neutral, like everything the agent sees.

`forge apply` (either level) writes its definition into Claude Code's user config directory (`agents/quality-reviewer.md`; §7.7), removes the files of former agents (`forge-reviewer.md`, `forge-workflow-step.md`, `quality-workflow-step.md`) when they are its own, and never overwrites a person's agent of the same name (apply stops and says so): `model` is `FORGE_REVIEW_MODEL` as it is when `forge apply` runs (else `inherit`, the session's), `effort` is `FORGE_REVIEW_EFFORT` (else `medium`, Claude Code's default for the current models: without it the agent would take the session's level, a harness's `--effort max` included), `background: false` (a foreground agent holds the main agent until it returns), and a fixed short system prompt: the task is the message the agent receives.

- **Forge writes the prompt.** PreToolUse of the main agent's Agent call for the reviewer answers with `updatedInput`: the `prompt` replaced by the waiting review's, `model` removed, `run_in_background` false. The main agent starts the reviewer but never shapes what it is told; it starts with a fresh context and sees no conversation.
- **Its own calls** (PreToolUse with its `agent_id`): Edit, Write, MultiEdit and NotebookEdit, and every Serena tool but its navigation, are refused ("You never change the project's files."). Past its 170 s every call is refused ("Time is up: end now with your answer as it stands."), so it ends with its answer.
- **Forge's other hooks** fire on its calls too, since it runs in the session: the Bash guard (§9.2) and the internals refusal (§9.3). It never gets the test-edit ask, the edit-time checks or a review of its own asks.
- **SubagentStart** links the reviewer to the oldest waiting review and starts its clock. **SubagentStop** checks its answer against the verdict schema, sending a refused one back with `decision: "block"` (three answers at most).
- **The answer stays Forge's.** The reviewer's last message is what the main agent's Agent call returns, and its verdict is not the main agent's to read. Once its answer is recorded, it is sent back once with "Your answer is recorded. End now with only the word: done.", every tool call refused meanwhile; its next stop ends it, and the main agent reads "done". A newer Claude Code's hand-back (`SubagentHandback`) is its answer the same way, and goes through as "done". PostToolUse of the main agent's Agent call says "The review is recorded: retry the same call unchanged."
- **No messages.** Claude Code's `SendMessage` reaches a finished subagent, which would let the main agent ask the reviewer for its answer. Forge records its reviewers' ids as they start (`agents/ids`) and refuses a `SendMessage` to one of them ("That agent worked for this project's tooling and has finished: it takes no messages."), and any `SendMessage` of theirs ("This agent sends no messages: end with your answer."). Messages between the session's other agents (concurrent work) are left alone.
- State lives in the session's directory (`agents/ids`, `agents/reviews/`, `agents/closing/`), never in the repository.

### Tests

- 8.1 Who reads what: `crates/forge-hooks/tests/events.rs` (every test asserts agent output names no internals; `stop_blocks_on_findings_not_on_notes`, `outside_a_project_hooks_are_silent`), `crates/forge-hooks/tests/spec_hooks.rs::a_checker_crash_is_silent_and_noted`, `crates/forge-cli/tests/hook.rs::hooks_on_an_applied_sample` (garbage input), `crates/forge-hooks/src/input.rs (unit)`.
- 8.2 Entries: `crates/forge-hooks/tests/spec_hooks.rs::hook_entries_wire_every_event`, `crates/forge-render/src/hooks.rs (unit)`, `crates/forge-launcher/src/hook.rs (unit)`, `crates/forge-launcher/tests/hook_shells.rs`.
- 8.3 Project and state: `spec_hooks.rs::state_lives_outside_the_repository_readable_by_the_user_only`, `crates/forge-cli/tests/hook.rs::the_guard_follows_a_package_manager_switched_mid_session`, `spec_hooks.rs::notes_keep_the_fifty_newest`, `crates/forge-hooks/src/state.rs (unit)`, `crates/forge-hooks/src/rules.rs (unit)`, `crates/forge-hooks/src/proc.rs (unit)`, `crates/forge-home/src/lib.rs (unit)`.
- 8.4 SessionStart: `events.rs::session_start_gives_repository_context_only`, `events.rs::session_start_starts_the_engine_for_typed_rules`, `spec_hooks.rs::session_start_shows_a_missing_pinned_version_to_the_person_only`, `spec_hooks.rs::session_start_puts_uv_on_path_only_for_uv_or_unpackaged_projects`, `crates/forge-tools/tests/env_file.rs`.
- 8.5 UserPromptSubmit: `events.rs::prompt_snapshots_the_turn_silently`, `spec_hooks.rs::a_task_notification_neither_starts_a_turn_nor_replaces_the_request`, `spec_hooks.rs::a_prompt_before_claude_stopped_continues_the_turn`, `spec_hooks.rs::a_blocked_stop_keeps_the_turn_and_a_stop_let_through_ends_it`, `crates/forge-hooks/src/snapshot.rs (unit)`; the capabilities it gives: §20's tests. The engine resync: (no test: needs a running engine; covered only by reading).
- 8.6 Files outside the repository: `spec_hooks.rs::a_file_written_outside_the_repository_is_checked_with_the_projects_rules`, `spec_hooks.rs::a_file_a_command_writes_outside_the_repository_is_checked`, `spec_hooks.rs::claude_codes_own_files_outside_the_repository_are_not_checked`, `outside.rs (unit)`; with the real tools after `forge apply` (the project's ruff configuration, the formatter's fix written back, the 1 MiB limit): `crates/forge-cli/tests/hook.rs::a_script_outside_the_repository_is_checked_with_the_projects_tools`.
- 8.6 PreToolUse / PostToolUse: `events.rs::post_tool_use_reports_findings_on_the_changed_files`, `events.rs::a_batchs_findings_are_those_of_each_files_last_check`, `spec_hooks.rs::a_snapshot_past_its_time_is_skipped_and_left_to_stop`, `spec_hooks.rs::bash_changes_leave_the_users_index_alone_and_skip_tool_directories`, `spec_hooks.rs::bash_snapshots_beyond_the_fifty_newest_are_dropped`, `spec_hooks.rs::a_pack_that_became_active_gives_its_context_once`, `crates/forge-cli/tests/hook.rs::context_of_packs_activated_mid_session`, `hook.rs::context_that_no_longer_holds_is_retracted_once`.
- 8.7 Typecheck: `events.rs::typecheck_reports_new_errors_through_the_engine`, `crates/forge-cli/tests/engine_typecheck.rs` (a typecheck rule alone: SessionStart starts the engine, an added error is reported; skipped without uvx), `spec_hooks.rs::edit_typecheck_never_is_silent`, `crates/forge-cli/tests/hook.rs::a_pyproject_edit_relocks_a_stale_uv_lock` (a fake uv). The stream-json ancestor lookup: (no test: needs a real ancestor process with those arguments).
- 8.8 Stop: `spec_hooks.rs::stop_checks_the_turns_outside_files_once`, `spec_hooks.rs::a_blocked_stop_keeps_the_turn_and_a_stop_let_through_ends_it`, `events.rs::stop_blocks_on_findings_not_on_notes`, `events.rs::advice_blocks_once`, `spec_hooks.rs::stop_remembers_a_passing_tree`, `spec_hooks.rs::stop_without_a_turn_snapshot_checks_every_change`, `spec_hooks.rs::a_commit_during_the_turn_is_part_of_it`, `spec_hooks.rs::the_block_reason_is_capped_and_a_new_prompt_resets_the_count`, `spec_hooks.rs::tests_failing_before_the_turn_too_never_block`.
- 8.9 PermissionRequest, SessionEnd, Notification: `events.rs::notification_and_session_end`, `spec_hooks.rs::notifications_use_osc_9_where_the_terminal_reads_it`; reviewer tests under §9.
- 8.10 Environment variables: covered by the tests of the behaviour each one changes.
- 8.11 Budgets: as in the table.
- 8.12 Claude Code behaviour: the real-Claude-Code end-to-end job (§14); not by the unit suite.
