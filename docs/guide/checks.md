# Checks and hooks

## `.forge/check.sh`

One entry point for the Stop hook, the pre-commit hook and CI:

```sh
sh .forge/check.sh --changed   # files from git status: staged, unstaged, untracked
sh .forge/check.sh --all       # the whole project (CI)
```

It runs these steps in order and stops at the first one that fails. Every line it prints starts with `forge:<rule-id>`.

1. **Integrity** (`forge/integrity`): every owned file matches its hash in the lock, and no stray file sits in Forge's directories.
2. **Applied migrations** (`forge/applied-migration`): a file under `alembic/versions/` or `prisma/migrations/` that exists at `HEAD` was modified or deleted.
3. **Tests** (`forge/deleted-test`, plus the `no-increase` / `no-decrease` ast-grep rules): test files deleted since `HEAD`; more `skip`/`only`, or fewer `assert`/`expect`, than at `HEAD`.
4. **ast-grep `forbid` rules** on the files in scope.
5. **Format, lint, typecheck**: the `check` targets of those stages, with the project's own binaries (`changed_run` on the changed files with `--changed`).
6. **Migration checks**: only when a `when_changed` path changed, and only against a [local database](/guide/configuration#databases).
7. **Tests**: the `check` targets of stage `test`; with `--changed`, only the related tests (for pytest: the changed test files plus `tests/**/test_<module>.py` for each changed module; for Vitest: `vitest run --changed`).

A check whose tool is not installed prints `skipped: <tool> not installed` and does not fail. `check.sh` is self-contained: it does not depend on `.claude/`, so CI only needs `sh`, `git` and the project's tools.

## Project hooks

| Event | Script | Behavior |
|---|---|---|
| SessionStart (`startup`, `resume`, `clear`, `compact`) | `session-start.sh` | At most 20 lines: branch, number of changed files and the first 10, last 5 commits, active packs, and missing tools (reported once). |
| PreToolUse (`Bash`) | `guard.sh` | The project's [guard](#guard) rules. If the script itself is missing or fails, the tool call is blocked. |
| PostToolUse (`Edit\|Write`) | `post-edit.sh` | Formats the file silently, lints it and runs the `forbid` rules on it. Findings go back to Claude (≤ 20 lines, this file only). Nothing on success. |
| PostToolUse (`Edit\|Write`), async | `typecheck.sh` | The project type check in the background. Errors wake Claude (exit 2, ≤ 20 lines). Skipped while another run is in progress. |
| Stop | `stop.sh` | Runs `check.sh --changed` (below). |

All scripts are POSIX `sh` and read paths from the hook input, never from `CLAUDE_PROJECT_DIR`.

### Stop

1. If the working tree is unchanged since the last passing run (hash of `git diff HEAD` and the untracked files), exit.
2. After three blocks in a row in the session, hand over: `forge: check failed 3 times. Handing over to you.`
3. Run `check.sh --changed`. On failure, block with its first 20 lines as the reason, so Claude fixes the problem and tries again.

## Guard

`guard.rules` holds one tab-separated line per rule: `id`, `decision`, `pattern`, `description`. `guard.sh` matches each POSIX ERE against the JSON-escaped command in one `awk` pass (p99 under 20 ms with 50 rules) and answers the first `deny`, else the first `ask`:

```json
{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"forge:safety/git-force-push Force-pushing overwrites remote history. Use --force-with-lease if you must."}}
```

Commands that a permission prefix can express use a `permission` target instead. Known blind spots, by design: commands hidden behind variables, `eval` of built strings, and encodings other than `base64 -d | sh`.

## User hooks

| Event | Script | Behavior |
|---|---|---|
| PreToolUse (`Bash`) | `guard.sh` | The [safety](/packs/safety) guard rules, in every project. |
| Notification (`permission_prompt\|idle_prompt`) | `notify.sh` | A terminal notification: OSC 777, or OSC 9 in iTerm2 and WezTerm. |

## Pre-commit and CI

`forge apply` offers to create `.git/hooks/pre-commit` running `sh .forge/check.sh --changed`, only when no hook manager is present (`.husky/`, `lefthook.yml`, `.pre-commit-config.yaml`, `core.hooksPath`) and no hook exists. Otherwise `forge status` prints the line to add to your manager. `forge remove` deletes the hook it created.

In CI:

```yaml
- run: sh .forge/check.sh --all
```
