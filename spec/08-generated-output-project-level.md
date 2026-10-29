## 8. Generated output — project level

```
<repo>/
├── .claude/
│   ├── settings.json                     # appended entries only
│   ├── skills/forge-project/
│   │   ├── .claude-plugin/plugin.json    # {"name":"forge-project", ...}
│   │   ├── hooks/hooks.json
│   │   └── scripts/{lib.sh, checks.sh, session-start.sh, turn-start.sh, guard.sh, guard.rules, review.sh, bash-snapshot.sh, test-edit.sh, post-edit.sh, typecheck.sh, stop.sh}
│   └── rules/forge-<pack-id>.md          # one per active pack with context.md; "/" in the id becomes "-"
└── .forge/
    ├── check.sh
    ├── ast-grep/{sgconfig.yml, rules/*.yml, rule-tests/*.yml}
    ├── helpers/                          # scripts engine helpers and the types engines run (§13.4, §15.2), when used
    ├── forge.lock.json                   # for the CLI
    ├── manifest.sha256                   # owned-file hashes for check.sh (sha256sum format)
    ├── .gitignore                        # state/
    └── state/                            # not committed: tools' caches (tsc's build info)

~/.claude/forge-state/<id>/               # the hooks' state for this repository (<state> below)
```

**Hook state.** The hooks keep their state (turn snapshots, Stop results and counters, the saved prompt, review verdicts and their log, Bash snapshots, the typecheck lock) in `<state>` = `$FORGE_STATE_HOME/<id>` (`FORGE_STATE_HOME` defaults to `~/.claude/forge-state`; `<id>` is the first 16 hex digits of the sha256 of the repository root's physical path), created readable by the user only. It is outside the repository, so the agent's Bash, which the sandbox confines to the repository, cannot forge a verdict, the request a reviewer reads, an approval or a Stop result; `safety/forge-state` denies Bash commands naming it and `safety/forge-state-edit` denies `Edit` there (§13). `.forge/state/` keeps only tools' caches.

### 8.1 Hooks

**The generated checks and the engine.** `forge apply` renders the active rules' checks (tool wrappers, one set of functions per check, the grep rules, `forge_role`) followed by the check engine: the modules of `templates/engine/` that these checks and the scripts running them call, found by name (a module's `forge_*` functions and `FORGE_*` constants), and the modules those call in turn; `core` always. A project therefore carries only the engine its rules use. `.forge/check.sh` holds its own copy, because its integrity check (§10.1 step 1) runs code from the same hashed file; the hook scripts (session-start, post-edit, typecheck, test-edit) source `scripts/checks.sh`, written next to `lib.sh`, which they already source the same way.


All scripts are POSIX `sh`. They read file paths and the working directory from the hook input (`cwd`, `tool_input.file_path`), never from `CLAUDE_PROJECT_DIR`.

| Event | Matcher | Script | Mode | Timeout | Behavior |
|---|---|---|---|---|---|
| SessionStart | `startup\|resume\|clear\|compact` | `session-start.sh` | sync | 10 s | ≤ 20 lines: branch, number of changed files and first 10 paths, last 5 commits (`%h %s`, ≤ 72 chars), active packs, missing tools (once), what the installed tool versions cannot do (§16.3, once until it changes). Starts the types engines the active rules use, in the background (§15.3). |
| UserPromptSubmit | — | `turn-start.sh` | sync | 10 s | Snapshots the tree at the start of the turn, resets the Stop block counter (§10.2) and saves the prompt to `<state>/<session_id>.prompt` (for a review of test edits, §10.5). Prints nothing. |
| PreToolUse | `Bash` | `guard.sh` | sync | 180 s | Project `guard` rules. `onFailure: "block"`. An ask nobody can answer is reviewed (§9.3). |
| PreToolUse | `Bash` | `bash-snapshot.sh` | sync | 15 s | Snapshots the working tree for the files the command changes (§8.2). Prints nothing. |
| PreToolUse | `Edit\|Write` | `test-edit.sh` | sync | 180 s | Asks before an edit changes or removes a test that existed at the start of the turn (§10.5); reviewed when nobody can answer (§9.3). |
| PermissionRequest | — | `review.sh` | sync | 180 s | §9.3. Defers to the user-level `review.sh` when it exists. |
| PostToolUse | `Edit\|Write\|Bash` | `post-edit.sh` | sync | 60 s | On the edited file (Bash: the files the command changed, §8.2): format silently; apply the `fix_run` safe fixes of in-scope lint checks, and when they changed the file add `forge: auto-fixed <codes> in <file>; re-read it before editing it again` (codes from ruff's `--show-fixes`, per file, else "safe lint fixes"); lint it (a check with `fix_lints` takes the fix run's result as its lint: one ruff call; a lint `changed_run` that writes, like Biome's `check --write`, applies its safe fixes in the same run: a change beyond what `format_stdin` explains adds the same line); run ast-grep `forbid` rules on it. Each tool runs once with all the files in its scope. Added lines are computed after these writes. The checks get `FORGE_EDIT_BUDGET` seconds (45, under the 60 s timeout); past it they are stopped and the findings so far are returned after `forge: the checks after this edit stopped after <n>s; findings so far are below, and the Stop check covers the rest.` Findings on lines added during the turn (against the turn snapshot; before the first prompt, against `HEAD`) → JSON `{"hookSpecificOutput":{"hookEventName":"PostToolUse","additionalContext":"…"}}`, ≤ 20 lines, diagnostics for these files only. Nothing on success. |
| PostToolUse | `Edit\|Write` | `test-edit.sh` | sync | 10 s | The call ran, so its test edits were approved: they go to `<snapshot>/tests-approved`; the file goes to `<snapshot>/touched` (§10.5). Prints nothing. |
| PostToolUse | `Edit\|Write\|Bash` | `typecheck.sh` | `asyncRewake: true` | 300 s | Project typecheck, when the edited file (Bash: a file the command changed) is in a typecheck's scope. Only the errors the change added wake Claude (exit 2, stderr ≤ 20 lines): an error the file already had, before the turn (its base: the turn snapshot, else HEAD), stays silent, as at Stop (§10.1 step 4: compared by text without the location, so moved code keeps its errors); errors only in other files → silent. A check run by ty (Forge's, §15.2) is answered by the Python types engine (§15.3) while it runs or can start: ty server's diagnostics of the edited file now and of its base copy (in memory), from a server that sees the untyped packages as `ty check` does, so no `ty check` process runs per edit. When the engine cannot answer (not running and not startable, an error, `FORGE_EDIT_TYPECHECK_ENGINE=never`), `ty check` runs on the project, compared with the same run in the base tree, and a report says so (`(checked by ty check: <why>)`). Other typecheck tools (tsc, vue-tsc, ngc, astro) run their check, compared with the base tree the same way. Skips if one is already running (lock in `<state>`, taken over when its pid is gone, or when it has had no pid for a minute). Does not run where Claude Code would wait for it: Claude Code runs an `asyncRewake` hook in the background only in an interactive session or with streaming input, so under `claude -p "<prompt>"` (`CLAUDE_CODE_ENTRYPOINT=sdk-cli` and no ancestor process with `--input-format stream-json` or `--sdk-url`; the ancestors are looked up once per session, the answer kept in `<state>/<session_id>.rewake`) it exits 0 at once and the Stop check runs the typecheck; `FORGE_EDIT_TYPECHECK=always` runs it anyway, `never` skips it everywhere. |
| PostToolUseFailure | `Bash` | `post-edit.sh`, `typecheck.sh` | as above | as above | The same two hooks for a Bash command that fails or is interrupted: it may have changed files before stopping (a script that writes three files then fails on the fourth). Same checks and output, with `hookEventName` `PostToolUseFailure`. A failed Edit or Write changes nothing, so they are not matched. |
| Stop | — | `stop.sh` | sync | 600 s | §10.2. |
| SessionEnd | — | `session-end.sh` | sync | 10 s | Only when an active rule uses the types engine (§15.3): unregisters the session; the last session to end stops the engines. Prints nothing. |

### 8.2 Files changed by Bash

Agents also change files through Bash (heredocs, `sed -i`, `cat >`, `git apply`), so the per-edit checks also follow Bash commands:

1. **Before** (`bash-snapshot.sh`, PreToolUse `Bash`): in a git repository with `.forge/`, write the working tree's git tree id to `<state>/bash/<tool_use_id>.tree`. The tree comes from `git add -A . && git write-tree` with `GIT_INDEX_FILE` set to a copy of the repository's index, `GIT_OBJECT_DIRECTORY` set to `<state>/bash/objects` and the repository's objects as `GIT_ALTERNATE_OBJECT_DIRECTORIES`: the user's index and object store are never written. Tracked and untracked files count; ignored ones do not. A snapshot that takes longer than `FORGE_SNAPSHOT_TIMEOUT` (5 s) is recorded as `skipped`.
2. **After** (`post-edit.sh`, PostToolUse `Bash`, or PostToolUseFailure `Bash` when the command failed or was interrupted): snapshot again and list the added and modified files (`git diff-tree -r --no-renames --name-status`), outside `.forge/`, `.claude/`, `.git/`, `node_modules/` and `.venv/`. The list goes to `<tool_use_id>.files` for typecheck.sh; the files (the first 20, with a note when there are more) get the same checks as an edited file, and the findings one `additionalContext` of at most 20 lines. Formatting happens after the comparison, so its writes are not counted (the next command's snapshot starts from them). A `skipped` snapshot prints, once per repository, that the Stop check covers Bash changes there.
3. **typecheck.sh** (PostToolUse or PostToolUseFailure `Bash`, async) waits up to 60 s for that list, then runs when a listed file is in a typecheck's scope.
4. **Cleanup.** A command denied before it runs never reaches PostToolUse: each snapshot drops `.tree` and `.files` beyond the 50 newest (counted with shell builtins; `ls` and `rm` run only when there are more), each SessionStart those older than a day, and either drops the object directory once no snapshot is pending.

Not covered: writes by a `run_in_background` command after it returns, and anything outside the repository; the Stop check still sees them.

### 8.3 Permissions appended

`ask`: `Edit(.forge/**)`, `Edit(forge.yaml)`, `Edit(.claude/skills/forge-project/**)`, `Edit(.claude/rules/forge-*.md)`, plus `permission` targets of active project packs.

### 8.4 Context

- Each active pack with a `context.md` produces `.claude/rules/forge-<pack-id>.md` (`/` replaced by `-`): one line, "These are this project's defaults. When the task or the user explicitly asks for something different, do what they ask and point out the conflict; never override an explicit instruction with these defaults.", a blank line, then the same content; no `paths:` frontmatter.
- `context.md` is ≤ 10 lines, written as facts. A line never claims more than the checks enforce: a requirement names what is rejected, scoped as the check is ("`text()` with an f-string is rejected", not "SQL is always constant"), and leaves room for what the check allows.
- Context never enforces anything; enforcement is done by checks and hooks.

### 8.5 Hook latency budgets

Every hook runs on Claude's critical path (PostToolUse after each edit, PreToolUse before each command), so its cost is bounded and tested (`tests/spec/08-5-hook-budgets.test.ts`). A process start is the unit that matters: about 1 ms natively, about 25 ms on an emulated machine (an amd64 image on arm64), so a hook that starts 100 processes takes seconds there whatever it computes.

- **What the hooks do instead of starting processes.** Strings, paths, counts, JSON fields without escapes, version comparisons, adapter lookups and file replays are shell builtins (lib.sh: `forge_json`, `forge_dirname`, `forge_cat`, `forge_version_lt`, …); one temporary directory per hook (`$FORGE_TMP`), removed once; HEAD is read from the turn snapshot, not asked of git; each tool runs once for all the edited files; one ruff call applies the safe fixes and reports the lint findings that remain (`fix_lints`, §6); the facts cache (§9.4) answers what a tool is and has (version, rule codes, the uvx binary of a pinned tool, which is then run directly); the formatted base copy of a file, and whether formatting changes it at all, is kept per base (§10.1). The edit-time typecheck asks the running types engine (§15.3) for the edited file and its base copy; `ty check` runs cold only when the engine cannot answer, and the hook then says why.
- **Budgets.** Measured warm (facts cached, the types engine running, a turn snapshot taken) on the FastAPI sample after an edit of one file: the commands the hook starts by name (git and the utilities, counted through a `PATH` of logging stand-ins; the tools run by path, ruff from `.venv` and ast-grep from uv's cache, are not counted), and the median of five runs.

| Hook | Commands (budget) | Median (budget) |
|---|---|---|
| `guard.sh` | 3 | 100 ms |
| `bash-snapshot.sh` | 9 | 250 ms |
| `test-edit.sh` | 3 | 150 ms |
| `post-edit.sh` | 18 | 600 ms |
| `typecheck.sh` | 16 | 400 ms |
| `turn-start.sh` | 18 | 400 ms |
| `session-start.sh` | 10 | 500 ms |

The time budgets hold on a CI runner; on the emulated bench image every PostToolUse hook stays under 1 s.

