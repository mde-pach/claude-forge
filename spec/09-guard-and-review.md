## 9. Guard and review

The PreToolUse hook (§8) refuses the agent's access to Forge's internals, decides on Bash commands with the active `guard` rules (§6), and asks before an edit changes a test. An ask nobody can answer goes to an independent reviewer. This chapter is the `forge-guard` crate and the PreToolUse and PermissionRequest parts of `forge-hooks`.

### 9.1 Commands are parsed, never matched as text

The guard parses the command line with tree-sitter-bash into the **simple commands** it runs: each one's assignments, words with their quoting removed, redirections, the heredoc or here-string fed to it, the directory it runs in, and its **command positions**. Rules match simple commands, never the raw text.

- **Data stays data.** A quoted argument and a heredoc body fed to a command that is not a shell are one word of that command. `git commit -m "remove chmod 777"`, `gh pr create --body "… git push --force …"`, `echo 'rm -rf /'` and `cat > notes.md <<'EOF' … EOF` trigger nothing.
- **What runs is followed:** lists (`;`, `&&`, `||`), pipelines, subshells, `$(…)`, backticks, `<(…)`, the bodies of `if`, `while`, `for`, `case` and functions; `sh`, `bash`, `zsh`, `dash`, `ksh`, `ash`, `busybox sh` with `-c '…'`; `su -c '…'`; `eval`; `env -S` / `--split-string`; each `find -exec|-execdir|-ok|-okdir … ;|+`; the command `xargs` runs; and a shell fed text through a heredoc, a here-string or a pipe from `cat` or `echo` (`sh <<EOF`, `bash <<< '…'`, `echo '…' | sh`). Scripts inside scripts are followed 8 levels deep.
- **Command positions** are the command itself, then the command a wrapper runs after its own options (`sudo`, `doas`, `env`, `time`, `nohup`, `nice`, `ionice`, `exec`, `command`, `builtin`, `xargs`, `stdbuf`, `timeout`, `chronic`, `caffeinate`), then the command a runner runs (`npx`, `bunx`, `pnpx`, `bun x`, `npm exec|x`, `pnpm exec|dlx|<bin>`, `yarn exec|dlx|<bin>`, `npm|pnpm|yarn|bun run <script>`, `uvx`, `uv run`, `uv tool run`, `pipx run`, `poetry|pdm|pipenv|hatch|rye|pixi|conda|mamba|micromamba run`, `python <script>`, `python -m <module>`). `sudo -u bob nice -n 5 npx prisma migrate reset` has positions at `sudo`, `nice`, `npx` and `prisma`. `command -v x` runs nothing.
- **The directory.** `cd <dir>` and `pushd <dir>` move the directory later commands run in; `sudo -D`, `env -C` move the wrapped command's. A target holding `$`, `` ` `` or a glob, `cd -` and `popd` leave the directory unknown.
- A tree with a syntax error (an unterminated quote, a stray `)`) is not trusted: the text is split again by a plain tokenizer and the same rules apply to its commands. No input makes the guard panic.

### 9.2 Guard rules

A `guard` target (§6) has a decision, `deny` or `ask`, and a `command` (one structural form or a list, any of which matches), a built-in `check`, or both.

- **`command`** matches a simple command at one of its command positions. The fields are §6's: `name` (the basename, `*` wildcards), `sub` (the first positionals, each `a|b` alternatives or a `/regex/`), `args_any`, `last_arg`, `flags_any` (`--force` also matches `--force=x` but not `--force-with-lease`; `-f` also matches the cluster `-uf`), `flags_all`, `flags_none`, `value_flags`, `option_values`, `arg_regex` (with `stdin: true`, also the heredoc or here-string fed to the command), `env_any` (an assignment before the command, or `export NAME=…`), `wrapped_by`, `pipe_to` (`*`: any), `redirect_to`, and `words`: a regex over the command's words from its name, joined by single spaces, the escape hatch. Every field given must hold.
- **`check`** runs a built-in on the whole parsed command line. With a `command`, the check runs only when the command matched and then decides. Each check runs at most once per command line. Each check has its decision, from what it finds, and a target with the other one does not load (`invalid rule: the <check> check's decision is <decision>, not <other>`): `package-add` and `ephemeral-exec` are `ask` (adding or fetching a package may be the task's), `python-pm` is `deny` (its reason names the command to use instead).
  - `ephemeral-exec`: a package launcher that would fetch what it runs from a registry. `pnpm dlx`, `pnpx`, `yarn dlx`, `pipx run`, `pnpm create` and `yarn create` always; `npx`, `bunx`, `bun x`, `npm exec|x`, `uvx` and `uv tool run` when given a package option before the command name (`-p`, `--package`, `-c`, `--call`; uv's `--from`, `--with*`, index options), a spec that is not a bare name (`name@version`, `name==version`, a URL, `user/repo`), or a command not installed where it runs (`node_modules/.bin/<name>` or `node_modules/<name>/package.json` for the Node launchers, `.venv/bin/<name>` for uv, from that directory up to the repository root). `npm create` / `npm init <x>` and `bun create <x>` are judged as running `create-<x>`; `npm init` alone is not flagged. `deno run|x|serve` on an `npm:`, `jsr:` or URL module is flagged. A command name expanded at run time is flagged when the variable holds a launcher (`N=npx; $N cowsay`), or when its value is unknown and the line names a launcher. A launcher in a directory that cannot be known in advance is flagged.
  - `package-add`: an install that names a package: `npm|pnpm|yarn|bun install|i|add <pkg>`, `yarn|pnpm global add`, `pip install`, `python -m pip install`, `uv pip install`, `uv|poetry|pdm add`, `pipenv install`, `pipx install`, `uv tool install`, through wrappers and runners. Local paths, `-r`/`-c` files and `-e` name no package; `--dry-run` installs nothing; read-only subcommands (`pip show|list|freeze|download|index|inspect`, `npm ls|view|outdated`, `uv pip list|tree|show|freeze`, `uv tree`) are not installs.
  - `python-pm`: pip into the project's environment (`pip install`, `python -m pip install`, `uv pip install` with a package, a requirements file or a local path) when the project's `python.pm` fact (§5) is `uv` or `none`. An install aimed at another environment passes: `--target`, `--prefix`, `--root`, `--system`, `--python`/`-p` outside the repository, a pip or python run by a path outside the repository, `VIRTUAL_ENV=` outside the repository, or a command run from outside the repository.
- **The decision** is the first deny target that matches, in rule order; else the first ask target; else nothing, and the command goes on to Claude Code's permissions.
- **The reason** the agent reads is the target's `reason`, else the rule's description, then ` Here: <what the check found>.` when a check found something. It never holds the rule id or anything about Forge. Examples:
  - `npx some-unknown-tool`: "… Here: some-unknown-tool is not installed where the command runs (no node_modules/.bin/some-unknown-tool in the repository root), so npx would fetch it from a registry and run it."
  - `pipx install requests flask`: "… Here: it adds requests and flask."
- **pip in a uv or unpackaged project** is denied with the check's own reason:
  - `python.pm` `uv`: "This project uses uv: `uv add <pkg>` (`--dev` for tooling). Here: `uv add requests`." (`-r <file>` carried over; a local install only: "Here: `uv sync` installs the project itself.")
  - `python.pm` `none`: "This project has no Python packaging yet: create it with `uv init --bare`, then `uv add <pkg>` (`--dev` for tooling). Here: `uv init --bare && uv add requests`."
  - Read-only pip commands and `--dry-run` pass. In a Poetry, PDM or Pipenv project pip is an ask of `package-add`, not a deny.
- **Project and user level.** `pre-tool-use` runs the project's guard rules (the lock's); `user-pre-tool-use` runs the user-level setup's rules (`~/.claude/forge-user.lock`) in any directory, with the project's package-manager facts when it runs inside a project. A deny or ask stops there; the Bash snapshot is taken only for a command that is not denied.

### 9.3 Forge's internals are refused

Every PreToolUse call is checked first. A tool call that names a path inside Forge's cache root or state root (§4) is denied with this reason, the same for every tool:

> Internal files of Forge, this project's quality tooling, used by Claude Code hooks; its checks run automatically on your edits; nothing here is needed for your task.

- Read, Edit, Write, MultiEdit: `file_path`. NotebookEdit: `notebook_path`. Grep and Glob: `path`, and a `pattern` that starts with `/` or `~`. Bash: any word or redirection target holding `/` or starting with `~` (for `NAME=value`, the value), resolved against the directory it runs in, when the command mentions `forge` or a root's path.
- `~`, `~/`, `$HOME/` and `${HOME}/` are expanded. A path matches lexically (`..` resolved) or physically (through symlinks, a symlink in the repository pointing into the cache included).
- Nothing points the agent at these paths, so the refusal should rarely fire. The PreToolUse matchers are `Bash` and `Read|Edit|Write|MultiEdit|NotebookEdit|Grep|Glob`, at project and at user level; at user level the second entry runs only this refusal.

### 9.4 Edits to tests ask

At project level, an Edit, Write or MultiEdit of a file in the scope of a rule whose check runs the `forge_test_edits` builtin (§6) is compared at three points: the file at the start of the turn (the turn snapshot's copy, else HEAD's), now, and after the edit (Claude Code's `old_string`/`new_string`, `replace_all`, `edits`, or `content`). A test that existed at the start of the turn and that the edit removes, or changes beyond formatting, makes the hook ask:

> This edit changes or removes tests in `<file>` that existed before this turn: "`<test>`" (changed), "`<test>`" (removed). Make it only if the task changes the behaviour these tests assert; otherwise leave the tests as they are and change the code instead.

New tests, reformatting and test support code (fixtures, fakes, helpers) do not ask; Stop reports changed support code (§10). The tests asked about are pending for the tool call (`<turn>/tests-pending/<tool_use_id>`). When the call runs, PostToolUse moves them to `<turn>/tests-approved`, which Stop does not block on.

### 9.5 The reviewer: asks nobody can answer

In an autonomous run an ask is otherwise denied without anyone looking at it. Forge then asks an independent reviewer.

- **When.** Never inside a review (`FORGE_REVIEWER` set) or in `dontAsk` permission mode. `FORGE_ASK_REVIEW=always` reviews every ask; `never` none. Otherwise an ask is reviewed when `CLAUDE_CODE_ENTRYPOINT` is `sdk-cli` (`claude -p`), `sdk-ts`, `sdk-py` or `claude-code-github-action`. The interactive CLI (`cli`), remote and desktop sessions count as having a person there; `FORGE_ASK_REVIEW=always` covers unattended remote runs, and an SDK host with its own approval UI sets `never`.
- **What is reviewed.**
  - A guard ask (PreToolUse): in `claude -p` a PreToolUse `ask` is denied without a PermissionRequest, so the guard reviews it itself. The reviewer judges only the rule's concern (its reason, with the check's finding) and the rule's `review` question, else "Is this action, as far as that concern goes, normal work on this repository and safe to run unattended?"; the rest of the command is covered by its own rules.
  - A test-edit ask (PreToolUse): the same, with the user's request for the turn (the saved prompt, else `(not recorded)`) in `<request>` tags.
  - A permission prompt (PermissionRequest): the action alone, against a fixed brief (allow only normal, safe work; deny anything irreversible, publishing, secret-reading, unverifiable installs, weakening security or tests, or code fetched from the internet; when in doubt, deny). The project's `permission-request` defers to `user-permission-request` when the user-level setup exists, so a request is reviewed once.
- **The reviewer** is a fresh, context-free Claude Code session that sees the repository, the tool and its input, never the agent's conversation:

  ```text
  claude -p <prompt> --output-format json --json-schema <{decision: allow|deny, reason}>
    --tools Read,Grep,Glob --allowedTools Read,Grep,Glob --permission-mode dontAsk --max-turns 12
    --no-session-persistence --strict-mcp-config --setting-sources "" --settings {"disableAllHooks":true}
    [--model $FORGE_REVIEW_MODEL]
  ```

  It runs in the hook's directory with `FORGE_REVIEWER=1`, capped at 170 s.
- **The answer.**
  - PreToolUse: `allow` or `deny`, with the reason "Reviewed by an independent session, nobody being there to answer: <reason>".
  - PermissionRequest: `{"behavior": "allow"}`, or `{"behavior": "deny", "message": "Denied by an independent review, nobody being there to answer: <reason>"}`.
  - No `claude` to run, a failed run or no verdict: nothing changes. The ask stands, and a PermissionRequest gets no answer.
- **Cache and log.** A PreToolUse verdict is kept per session (`<session>/review-cache`) for the same rule, tool input, concern and request, whitespace aside; a new request for the turn asks again. Every verdict reached is logged to `<state>/<repo-id>/reviews.log`, or `<state>/user/reviews.log` outside a project, one line per verdict: time, session, `allow` or `deny`, tool, input, reason, tab-separated.

Verified on Claude Code 2.1.308 (not documented by Claude Code): `claude -p` sets `CLAUDE_CODE_ENTRYPOINT=sdk-cli` and the interactive CLI `cli`; PermissionRequest fires in `-p` in default, auto and bypassPermissions modes but not in dontAsk; a PreToolUse `ask` in `-p` is denied without a PermissionRequest; a PreToolUse `allow` does not override a settings `deny`; the deny `message` field of a PermissionRequest answer is accepted.

### Tests

- 9.1 Parsing: `crates/forge-guard/src/parse.rs (unit)`, `crates/forge-guard/tests/corpus.rs::quoted_and_heredoc_text_never_triggers`, `corpus.rs::text_fed_to_a_shell_is_parsed`, `corpus.rs::unparseable_input_is_handled_conservatively`, `crates/forge-guard/tests/spec_parsing.rs` (`every_shell_dash_c_is_parsed`, `pushd_moves_the_directory_like_cd`, `popd_and_cd_dash_leave_the_directory_unknown`, `xargs_runs_its_command`).
- 9.2 Guard rules: `crates/forge-packs/tests/loading.rs::a_guard_check_takes_its_own_decision`, `crates/forge-guard/tests/corpus.rs` (`corpus_with_the_pack_rules`, `reasons_are_neutral`, `check_findings_in_reasons`, `pip_in_a_uv_project_is_denied`), `crates/forge-guard/tests/examples.rs` (every guard rule's pack examples), `crates/forge-hooks/tests/events.rs::pip_install_in_a_uv_project_is_denied_with_uv_add`, `events.rs::user_guard_reads_commands_not_messages`, `crates/forge-hooks/tests/spec_hooks.rs::the_user_guard_applies_in_any_directory`, `crates/forge-cli/tests/hook.rs::hooks_on_an_applied_sample`.
- 9.3 Internals: `events.rs::forge_files_are_refused_neutrally` (each tool of the file-tools matcher), `events.rs::the_user_level_refuses_forge_files_to_every_file_tool`, `crates/forge-home/src/lib.rs (unit)::internal_paths`, `hook.rs::hooks_on_an_applied_sample`.
- 9.4 Test edits: `events.rs::test_edits_ask`.
- 9.5 Reviewer: `events.rs::reviewer_answers_asks_nobody_can`, `spec_hooks.rs::a_guard_ask_nobody_can_answer_is_reviewed`, `spec_hooks.rs::the_reviewer_runs_a_fresh_read_only_claude`, `spec_hooks.rs::reviews_outside_a_project_are_logged_in_the_user_state`, `spec_hooks.rs::a_test_edit_verdict_is_not_reused_once_the_request_changed`. The "verified on 2.1.308" facts: the real-Claude-Code end-to-end job (§14).
