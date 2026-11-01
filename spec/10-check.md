## 10. The checker

The checker (crate `forge-check`) runs the active rules on a change. It is what `forge check` runs for people, CI and the commit-time check (§7), and what the PostToolUse and Stop hooks run for the agent (§8). Everything a run says is a finding (for the agent) or a note (for the person); §10.9.

### 10.1 `forge check`

```
forge check [--changed | --since <ref> | --all] [--json]
```

| Mode | Used by | Changed files | Base |
|---|---|---|---|
| `--changed` | the commit-time check (§7), a person | uncommitted paths: staged, unstaged and untracked (`git status`) | `HEAD` |
| `--since <ref>` | CI on a pull request | `git diff --name-only <base>`, plus untracked files | `git merge-base <ref> HEAD` |
| `--all` (the default) | CI on the default branch | every project file: tracked, and untracked but not ignored | `HEAD`, for the counts |
| `--turn <dir>` | the Stop hook (§8), through the library | the files whose content differs from the turn snapshot `<dir>` | the tree at the start of the turn |

- The modes exclude each other.
- `forge check` must run inside a git repository. It selects the rules as `forge apply` would (§3): the packs (built-in and local), local rules, `forge.yaml` and detection. It needs no lock.
- Paths are listed NUL-separated, so a non-ASCII file name is taken as it is.
- Never part of a change or of `--all`: `.forge/`, `.claude/worktrees/`, and any path inside a `.git`, `node_modules` or `.venv` directory.
- `--since` a ref that has no merge base with `HEAD` fails with `forge check: no merge base for <ref> and HEAD (fetch it: git fetch origin <branch>, or check out with fetch-depth: 0)`.
- `forge check` may install a missing pinned tool (§17) unless `FORGE_NO_DOWNLOAD` is set, and starts the types engine when a rule needs it (§15).
- Its project checks share a budget of 5 minutes (§10.4).

**Exit status.**

| Status | Meaning |
|---|---|
| 0 | pass |
| 1 | a finding fails |
| 3 | the only failures are tests that also fail at the start of the turn (§10.6) |
| 4 | advice was the only thing reported (§10.7) |
| 2 | the check could not run: not a repository, no merge base, the packs did not load |

**Output.** Without `--json`, `forge check` prints each finding with its rule id (`<rule-id> <path>:<line> <message>`), errors first, then advice, then `Notes:` and one note per line (`<rule-id>: <text>`). With `--json` it prints one object: `status` (`pass`, `fail`, `unverified`, `advice-only`), `exit`, `findings` (`rule`, `severity`, `path`, `line`, `message`, `agent_text`), `notes` (`kind`, `rule`, `text`), `unverified`, `timings_ms` (`forge`, `tools`).

### 10.2 The change and its base

The change is computed once per run: the changed paths, which exist now, which were deleted since the base, a copy of each changed file as it is in the base, and the lines each existing changed file added.

- **Base copies** come from the base commit (`git cat-file --batch`) or, for a path that was uncommitted at the start of the turn, from the turn snapshot. A turn's base blobs are kept in the snapshot, so later edits of the turn ask git nothing.
- **Added lines** are what `git diff --no-index -U0` reports against the base copy (Myers diff with git's slider heuristics, computed in process). A file absent from the base counts fully. An edited line counts as added, and so does moved code.
- **Formatting.** A formatter rewrites whole files, so it may touch lines the change never did. When a check in a file's scope has a `format_stdin`, the base copy is also formatted with it, and a line counts only when it is added against both the base copy and the formatted copy; a line added against the base whose text is that of a line added against the formatted copy also counts (each such line once). When no filter applies, its tool is missing, or it fails or prints nothing on the base, the plain comparison applies. The formatted copies are kept in the turn snapshot and dropped when a formatter config at the root (`pyproject.toml`, `ruff.toml`, `biome.json`, `.prettierrc*`, `.editorconfig`, `package.json`, …) is newer than them.
- **Branch base.** A check target with `base: branch` (§6.2) is about the branch as a whole. At Stop (`--turn`) and with `--changed`, it still runs only when the run's own change touches its scope (§10.3), but its builtins get the branch's change: `FORGE_CHANGED` lists the files that differ from the merge base of HEAD with the default branch (committed on the branch, uncommitted, untracked), plus the run's changed files, and `FORGE_BASE` holds their copies at that merge base. The default branch is what `origin/HEAD` points to, else the first of `main`, `master`, `origin/main`, `origin/master` that exists; on the default branch itself the merge base is HEAD. A finding about a file the run did not change is dropped (earlier commits of the branch are not this change's); a finding with no path stays. With no default branch or no merge base, the check compares with the run's own base, with the note `compared with <base> instead of the branch's start: <why>`. `--since` already compares with a merge base, `--all` with nothing, and after an edit the run's own base applies. Tests: `crates/forge-cli/tests/check.rs::a_chart_is_bumped_once_per_branch`, `::a_migration_the_branch_added_may_be_edited`.
- **Turn snapshot.** The snapshot directory holds `head` (the commit at the start of the turn, possibly empty), `dirty` (`<path><TAB><blob hash>` per path uncommitted then, `-` when it was deleted), `untracked` (the dirty paths git did not track then, one per line; the guard reads it, §9.2) and `files/<path>` (a copy of each dirty file). Under `--turn` the change is what differs from the snapshot, commits made during the turn included; the person's own uncommitted work from before the turn is not part of it. §8 says when the hooks write it.

### 10.3 Steps

At Stop and with `forge check`, the steps run in order, and the run stops after the first step that fails:

1. **Tests and counts.** A deleted test file fails (`<path> This test file was deleted: restore it (removing tests is the user's decision: ask them).`, rule `forge/deleted-test`). Test files are the paths matching `test_*.py`, `*/test_*.py`, `*_test.py`, `conftest.py`, `*/conftest.py`, `tests/*`, `*.test.*`, `*.spec.*`, `*/__tests__/*` or `e2e/*`, where `*` also matches `/`. Then every `no-increase` and `no-decrease` rule (ast-grep and grep) compares its total over the changed files, deleted ones included (counted at the base), with the base's: moving a match to another file is no change, deleting the file that held it is. A failure reads `<path> <description> (<before> -> <after>)` when one file's count moved, else `<description> (<before> -> <after>) in <paths>`.
2. **Forbid rules.** ast-grep and grep `forbid` rules on the added lines (every line with `--all`): `<path>:<line> <description>`. An ast-grep match counts when its range overlaps an added line. `ast-grep-ignore` comments apply. A rule with `where` keeps only the matches the types engine confirms (§15); without the engine it is a note (`not enforced: its matches need the types engine, which is not available`). A rule that does not compile is a note.
3. **Check stages**, in this order: `format`, `lint`, `codegen`, `typecheck`, `migration`, `test`. Each stage is a step: the run stops after a stage with a failing check, so a failing `codegen` check (stale generated code) stops before the typecheck, whose errors would only repeat it.

Advice never stops the steps after it.

**Which checks of a stage run.**

- Outside `--all`, a check runs only when one of its in-scope files changed; with `when_changed`, only when a changed file matches it (and then even with no in-scope file).
- A check whose tool is missing is skipped with the note `skipped: <tool> not installed` (§10.9). Tools are the project's own (`node_modules/.bin`, `.venv/bin`, the project's Python environment), system tools on PATH (`uv`, `bun`, `docker`, `npm`, `pnpm`, `yarn` from Yarn 2, `helm`, `kubectl`, `tofu`; `terraform` is `tofu` in an OpenTofu project; `kustomize` falls back to `kubectl kustomize`), or Forge's pinned tools (§17).
- A check with `requires_local_db` runs only when the database is on this machine: `DATABASE_URL` from the environment, else from `.env`, else `sqlalchemy.url` in `alembic.ini`, with host `localhost`, `127.0.0.1`, `::1`, empty, or a Unix socket. Otherwise it is skipped with the note `skipped: no local database`.
- With `--changed`, `--since` and `--turn`, a check with a `changed_run` runs it on its in-scope changed files (`{files}`); otherwise it runs `run`, for the whole project.
- A check whose rule's pack was detected only in some workspace packages runs in each of them, with its files relative to the package; paths in its output are prefixed with the package directory (§5).

### 10.4 A check's result

- **Exit 0** passes.
- **Its own `timeout`.** A check past it is stopped with its process group and fails: `timed out after <n>s and was stopped (a hung test, or a service that never answers?); its output until then:` and an excerpt.
- **The run's budget.** The project checks of one run share a budget: 5 minutes at Stop and for `forge check`, 45 seconds after an edit. A check the budget stops passes with the note `not checked: the run's time budget ran out after <n>s, before this check finished`. Forge's own time is not counted.
- **A check that cannot start** passes with the note `not checked: the check could not start (<why>)`.
- **Attribution.** Outside `--all`, the diagnostics of a failing `lint` run count on the added lines, and those of a failing `typecheck` run (or a whole-project `format` run) in the changed files. Locations are read from the tools' plain output: `path:line[:col]`, `path(line,col)`, `--> path:line:col`, ESLint's stylish blocks (an absolute path alone on a line, then `line:col  error|warning  …` lines), `Would reformat: path`, Biome's `path format`, Prettier's `[warn] path` and `[error] path: …`. Absolute paths under the repository are made relative.
  - Nothing counts → the check passes with a note: `ok for the added lines (findings remain on lines this change did not add)` or `ok for the changed files (errors remain in unchanged files)`.
  - Only warnings and no error → it passes with the note `ok: only warnings`. With `fail_on_warnings`, warnings count as errors.
  - **What the change did not add.** A failure that names no added line — no location, or findings about a whole changed file (no line, or line 0), or a stage whose output is not attributed — is run again on the base tree (§10.1 step 4) at Stop and with `forge check`, and only the output lines the base did not print count (lines compared with their digits left out, so a count or a line the edit shifted is not new). Nothing new: it passes with the note `ok for this change: what it reports was already there at <base>`. The base run passing: everything counts. No base tree: the note `could not be compared with <base>: <why>; all of it counts`, and everything counts. After an edit there is no base run: such a failure is the note `failed naming no added line; the Stop check compares it with the base`. A builtin that compares with the base itself marks its findings as the change's, and is not run again. The base run's `FORGE_ROOT` is the base tree. Tests: `crates/forge-check/tests/spec_check.rs::a_failure_the_base_already_had_is_not_the_changes`, `::a_whole_file_finding_the_base_already_had_is_not_the_changes`, `::the_base_run_names_the_base_tree_as_its_root`, `::counts_are_totals_deleted_files_included`.
  - Only Forge's own files are named → it passes with a note.
  - Otherwise the diagnostics that count are the findings, at most 15, then `... and <n> more`.
  - `--all` fails on any error.
  - A builtin that created `FORGE_ATTRIBUTED` (§6.4) compared with the base itself: its findings count as they are, wherever they are, at most 15.
- **Dependencies not installed.** A `typecheck` run's `unresolved-import` diagnostic (ty's ``Cannot resolve imported module `<m>` ``) for a module that a dependency the project declares provides (`pyproject.toml`, Poetry's tables, `requirements*.txt` and `requirements/*.txt`, `Pipfile`) is the environment's state, not the change's: it is dropped from the output, and the note `dependencies are not installed in the environment Forge type-checks with: <dist> (<module>), …; their imports are not checked until they are` tells the person. A distribution's modules come from its installed metadata in the project's environment (`VIRTUAL_ENV`, else `.venv`: `top_level.txt`, else `RECORD`), else from its name (lower case, `-` and `.` as `_`, and a table of the common names that differ: `beautifulsoup4` → `bs4`, `pyyaml` → `yaml`, `pillow` → `PIL`, `scikit-learn` → `sklearn`, `python-dateutil` → `dateutil`, `opencv-python` → `cv2`, …). A run with no error left passes. An import of a package the project does not declare stays a finding.
- **Typecheck against the base.** A `typecheck` run with errors in changed files runs again in a copy of the base tree. Diagnostics are compared by their text without the location (`a.py:12:5: error[x] msg` is `error[x] msg`): of the errors in changed files, only as many of a text count as the run now reports more of it than the base did, in the whole project (moved code brings its errors along), those on added lines first. None counts → pass. Otherwise the findings are the errors that count. The errors that were already there are the person's, not the agent's: the note `<n> error(s) in the changed files were already there before this change`. When the base cannot be checked, every error in the changed files counts, with the note `its errors in the changed files could not be compared with <base>: <why>; all of them count`.
- **A failure that is not attributed** (codegen, migrations, tests, a crash) becomes findings: when its output names locations, one finding per location; otherwise one finding holding the stage's header and an excerpt of at most 15 lines (25 for tests). The headers are `The formatting check failed:`, `The lint check failed:`, `The generated code is not up to date:`, `The type check failed:`, `The migration check failed:`, `Tests failed:`. An empty output reads `(it printed nothing)`. The excerpt shows output that fits as it is; otherwise the failure first (test summaries, failing tests and their errors, or the lines naming an error), then the rest.

### 10.5 Config ratchet and accepted changes

The `python/config-ratchet` and `typescript/config-ratchet` rules run `forge_ratchet`: each changed tool config is compared with its base copy, and each loosened setting is a finding, followed by `Restore the setting, or ask the user to accept this change.` Python configs: `pyproject.toml`, `ruff.toml`, `.ruff.toml`, `ty.toml`, `mypy.ini`, `.mypy.ini`, `pyrightconfig.json` (ruff, ty, pytest, coverage, mypy, pyright, basedpyright settings). JavaScript configs: `biome.json`, `biome.jsonc`, `tsconfig*.json`, and `package.json` check scripts. Other rules a person may accept end with `Restore it, or ask the user to accept this change.` (`forge_accept_hint`).

**Project setup is not the change's.** `forge_uv_cooldown` (`python/uv/exclude-newer`) checks `uv.toml`, else `pyproject.toml`'s `[tool.uv]`. With `--all` it reports every missing or weak setting. In a change (`--changed`, `--since`, the turn) it reports a setting only when the change made it missing or weak: the file changed and its base copy had it right, or the change created the file. A setting the project already lacked is its setup, which `forge apply` offers (§3.5) and `forge status` reports, not the agent's to fix. A `pyproject.toml` the agent's turn created gets the missing settings from Forge, as `forge apply` adds them (`exclude-newer = "7 days"`, `required-version = ">=0.9.17"` under `[tool.uv]`): they are mechanical, so the turn reads the advice `pyproject.toml: added [tool.uv] exclude-newer and required-version.` instead of a finding; a value the file set is kept and judged. In a commit (`--changed`) and in CI nothing is written, and the missing settings are findings. Tests: `crates/forge-cli/tests/check.rs::the_uv_cooldown_blocks_only_what_the_change_broke`, `::the_uv_cooldown_is_added_to_a_created_pyproject`. The JavaScript package managers' cooldowns (`typescript/<pm>/release-age`) work the same way: `forge_npm_cooldown` (`.npmrc` `min-release-age` in days, and package.json requiring npm >= 11.10 through `devEngines.packageManager` or `packageManager`), `forge_pnpm_cooldown` (`pnpm-workspace.yaml` `minimumReleaseAge` in minutes, read from pnpm 10.16), `forge_yarn_cooldown` (`.yarnrc.yml` `npmMinimalAgeGate`, minutes or a duration such as `3d`, read from Yarn 4.10) and `forge_bun_cooldown` (`bunfig.toml` `[install] minimumReleaseAge` in seconds); for pnpm and Yarn a `packageManager` pin below the first release that reads the setting is a finding too. Each reads only the current files and their base copies, and its findings are the change's as they are (§10.4). Tests: `crates/forge-cli/tests/check.rs::the_uv_cooldown_blocks_only_what_the_change_broke`, `::the_js_cooldowns_block_only_what_the_change_broke`, `crates/forge-builtins-code/src/cooldown.rs` (unit).

**A file's own convention.** `forge_pinned_requirements` (`python/pip/pinned-requirements`) holds an entry a change adds to a requirements file to the file's convention: an added bare name or range (`requests`, `django>=5.0`) is a finding on its line when the file's base copy pins every entry, or the change created the file. A file that writes ranges or bare names keeps doing so, and the whole project (`--all`) has no change to judge, so nothing is reported there. Test: `crates/forge-cli/tests/check.rs::requirements_keep_their_own_convention`.

**A derived lock is kept, not reported.** `forge_uv_lock` (`python/uv/lock-sync`) runs `uv lock --check` when the change touched `pyproject.toml` or `uv.lock`, and `forge_js_lock <pm>` (`typescript/<pm>/lock-sync`) the package manager's `lock-check` (§16) when it touched a `package.json`, the lock or the package manager's config. A stale lock is relocked with `uv lock`, or the adapter's `lock` operation, which writes only the lock (`npm install --package-lock-only --ignore-scripts`, `pnpm install --lockfile-only --ignore-scripts`, `yarn install --mode=update-lockfile`, `bun install --lockfile-only --ignore-scripts`): in the turn, the advice `<lock> was updated to match <manifest>.` (`uv.lock was updated to match pyproject.toml.`, `pnpm-lock.yaml was updated to match package.json.`); in a commit (`--changed`), the finding `… Add it and commit again.`, which refuses the commit once with the new lock in the tree; in CI (`--all`, `--since`) nothing is written, and a stale or missing lock is a finding. A change is not held to a lock the project never had (no lock before or after): creating it is setup, which `forge apply` offers (§3.5). A manifest that does not resolve is a finding with the tool's error. A release that can check the lock but not update it alone (Yarn 2.4) leaves the stale lock a finding, with a note; one that cannot check it without installing (Bun before 1.1.43, pnpm before 8.6.12) is a note.

**New dependencies first.** The lock-sync checks run in the `codegen` stage, after the `lint` stage that holds the new-dependency checks (`supply-chain/new-npm-dep-exists`, `supply-chain/new-pypi-dep-exists`): the run stops after a failing stage (§10.3), so a dependency name that does not exist, is deprecated or is younger than the cooldown is never resolved into the lock; the relock happens at the next Stop once the name is fixed. After an edit, the background hook relocks uv's lock too, except when the edit gave `pyproject.toml` a dependency name its copy at the start of the turn lacked (§8.7); the JavaScript locks are relocked at Stop and at commit only. `typescript/lockfile-updated` leaves a lock to its package manager's active lock-sync check, and names only that package manager's command. Tests: `crates/forge-cli/tests/check.rs::the_uv_lock_is_kept_in_sync`, `::the_js_lock_is_kept_in_sync`.

A person accepts a finding in `forge.yaml`, whatever its rule (a grep or ast-grep forbid, a `no-increase` / `no-decrease` count, a check's diagnostic, a builtin's finding, `forge/deleted-test`):

```yaml
accept:
  - rule: <rule-id>
    file: <glob>        # optional: the finding's file (§6.1 globs, repository-relative)
    change: <text>      # optional: the finding's text
    reason: string
```

An entry holds for a finding when all of these hold:

- `rule` is the finding's rule;
- `file`, when given, matches the finding's path. A count over several files (`<description> (<before> -> <after>) in <paths>`) needs every one of them to match; a finding with no path is never matched by an entry with `file`;
- `change`, when given, equals the finding's text as the agent reads it (`<path>:<line> <message>`), or its message alone (without `<path>:<line> `, which moves as the file changes), leading and trailing whitespace aside. A multi-line finding is matched by its whole message.

An entry names `file`, `change` or both; one with neither is refused when `forge.yaml` loads (`give file, change or both`): a rule accepted everywhere is disabled instead (`disable:`). An entry with only `file` accepts every finding of its rule in those files, until it is removed.

An accepted finding is dropped where it is found: an accepted count, forbid match or check failure no longer fails its step, so the later steps run, and a check whose findings are all accepted passes. A builtin also drops a finding whose text equals an accepted `change` of its rule (§6.4).

**A refused commit says how to accept.** When `forge check --changed` (the commit-time check, §7.6) fails, its text output (for a person; not `--json`, and never under `CLAUDECODE`) ends with the entry that would accept each failing finding: `{rule: <id>, file: "<path>", reason: "<why>"}` by rule and file, or `{rule: <id>, change: "<message>", reason: "<why>"}` for a one-line finding with no path, under `accept:`, with the line `To accept one of these instead of fixing it, add its entry to forge.yaml, with your reason (it then holds for that rule in that file):`. The agent never reads it: its text is made of findings only (§10.9), and its own edits of `forge.yaml` ask (§7.2). Tests: `crates/forge-check/tests/spec_check.rs::accepted_findings_of_every_kind_are_dropped`, `::a_refused_commit_says_how_to_accept`, `crates/forge-cli/tests/check.rs::a_refused_commit_names_the_accept_entry`.

### 10.6 Failing tests at the start of the turn

Under `--turn`, a failing `test` check with a `rerun` is compared with the start of the turn:

1. The failing tests are read from its output (pytest and Vitest formats).
2. Their files run again (`rerun`) in a copy of the tree as it was at the start of the turn.
3. A test failing now that passed then is a new failure: the finding reads `<n> failing test(s) passed at the start of the turn:`, the tests (at most 8), and an excerpt. The others, which also failed then, are a note: `<k> other failing test(s) also fail at the start of the turn, so the change did not cause them: <tests>`.
4. When every failing test also failed then, the check is **unverified** (exit 3). Those failures are not the change's, so they are no finding and never block Stop (§8.8): the note `<n> failing test(s) also fail at the start of the turn, so the change did not cause them, but it is not verified by them either: <tests>.` tells the person, naming the errors that look environmental (refused connections, failed DNS lookups, timeouts) and what to set up (`docker compose up -d` with the repository's compose file, when there is one), else `Run them once what they need is set up.` The run's `unverified` list holds `test<TAB><rule> <test id>` and `env<TAB><error>` entries.
5. When the comparison is not possible (the copy cannot be made, the re-run is stopped, or fails without naming a failing test), every failing test counts, with a note saying why.

### 10.7 Advice

Advice is a finding that never fails a run alone (exit 4 when it is all there is). An advised check's output goes through the same attribution as a failing one (§10.4): only what the change added is advice.

- `advise`-mode ast-grep and grep rules, after an edit only (§10.8);
- `forge_once <command>`: each line the command prints is a finding. Under `--turn` it is advice, given once per finding and turn (the turn snapshot's `reported` file); elsewhere any output fails the check;
- `forge_advise <command>`: `forge_once` under `--turn` only; the commit-time check and CI never run it;
- a builtin whose findings are all advice (`forge_condor` gives its advice under `--turn` only, once per finding and turn, like `forge_once`).

The agent reads advice as `<path>:<line> Advice: <message>`.

### 10.8 After an edit

The PostToolUse hook (§8) runs the checker on the files the tool changed, against the turn snapshot (before the first prompt of a session: against `HEAD`), with a budget of 45 seconds for the project's tools. Only `format` and `lint` checks run, plus the forbid and advise rules:

1. **Format**, silently: each `format` check's `changed_run` (when it has `{files}`) on the edited files in its scope.
2. **Safe fixes**: each `lint` check's `fix_run` on the edited files in its scope. A file the fixes changed gets the advice `<path> Advice: safe fixes were applied (<codes>); re-read the file before editing it again` (the codes from ruff's `--show-fixes`, else `safe lint fixes were applied`). A lint `changed_run` that writes (Biome's `check --write`) gets the same advice when it changed more than formatting explains.
3. **Lint**: each `lint` check's `changed_run` (`{files}`) on the edited files; a check with `fix_lints` takes its fix run's result instead. Findings count on the added lines, computed after these writes, at most 15 per check.
4. **Forbid and advise** rules on the added lines.

A check whose tool is missing is skipped silently. Typechecks run in the separate asynchronous typecheck hook (§8, §15). Missing pinned tools are never installed after an edit.

### 10.9 Findings and notes

Every output of a run is one of two types:

- A **finding** is for the agent: a problem in its change that it can fix. Its text is `<path>:<line> <message>` (`<path> <message>` without a line; the message alone without a path), with `Advice: ` before an advice message. It never holds the rule id. Hook output and Stop reasons are made of findings only (§8).
- A **note** is for the person: a missing tool, what an installed release cannot do, a failure of Forge itself, the environment. Kinds: `missing-tool`, `not-enforced`, `forge-failure`, `environment`, `setup`. Notes go to `forge check`'s output, and from the hooks to the state directory, which `forge status` prints (§3). They never reach the agent.

A failure of Forge itself (a rule that does not compile, a builtin that broke, a budget that ran out, an engine that did not start) is a note and never fails a check.

### Tests

- §10.1 modes and change sets: `crates/forge-check/tests/change_set.rs`, `crates/forge-check/tests/spec_check.rs::since_without_a_merge_base_says_what_to_fetch`, `crates/forge-cli/tests/check.rs`; exit codes and output: `crates/forge-check/tests/spec_check.rs::exit_codes`, `::person_text_json_and_agent_text`.
- §10.2 added lines: `crates/forge-check/src/change.rs` (unit), `crates/forge-check/tests/change_set.rs`, `crates/forge-cli/tests/check.rs::forbid_on_added_lines_only`. The `format_stdin` comparison: no hermetic test (it needs a formatter; the e2e samples in `crates/forge-cli/tests/e2e.rs` run it when ruff is installed).
- §10.3 steps: `crates/forge-check/tests/spec_check.rs` (`a_failing_count_stops_the_later_steps`, `no_decrease_counts`, `forbid_on_added_lines_and_never_in_prose`, `a_failing_codegen_stage_stops_before_the_typecheck`, `a_missing_tool_is_a_note_and_the_check_passes`, `when_changed_gates_a_check`, `a_typed_rule_without_the_types_engine_is_a_note`), `crates/forge-cli/tests/check.rs::deleted_tests_and_counts`, `crates/forge-check/src/builtins/machinery.rs` (unit: local databases).
- §10.4 results and attribution: `crates/forge-check/tests/spec_check.rs` (`a_check_past_its_timeout_fails_saying_so`, `a_spent_budget_is_a_note_not_a_failure`, `lint_is_attributed_by_added_line`, `self_attributed_findings_are_taken_as_they_are`, `warnings_count_only_with_fail_on_warnings`, `typecheck_errors_in_unchanged_files_are_a_note`, `typecheck_errors_the_base_already_has`, `an_uninstalled_declared_dependency_is_a_note`, `failure_text_and_caps`), `crates/forge-check/src/diagnostics.rs` (unit), `crates/forge-check/src/shell.rs` (unit), `crates/forge-check/src/pydeps.rs` (unit).
- §10.5 ratchet and accepted changes: `crates/forge-helpers/tests/golden.rs`, `crates/forge-helpers/src/ratchet.rs` (unit), `crates/forge-check/tests/spec_check.rs::builtin_protocol_accepts_and_notes`.
- §10.6: `crates/forge-cli/tests/check.rs::failing_tests_compared_with_the_start_of_the_turn`, `crates/forge-check/src/checker.rs` (unit).
- §10.7: `crates/forge-cli/tests/check.rs::advice_once_per_turn`, `crates/forge-check/tests/spec_check.rs::after_an_edit_advice_and_no_later_stages`.
- §10.8: `crates/forge-check/tests/spec_check.rs::after_an_edit_advice_and_no_later_stages`, `crates/forge-check/tests/change_set.rs::post_edit_overhead`, `crates/forge-check/src/checker.rs` (unit: ruff's fix summary), `crates/forge-cli/tests/e2e.rs`. The safe-fix advice and `fix_lints`: no hermetic test (they need ruff or Biome).
- §10.9: `crates/forge-core/src/lib.rs` (unit), `crates/forge-cli/tests/check.rs::check_examples` (no agent text names internals).
