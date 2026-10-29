## 1. Principles

Forge is one installed binary. It reads rules grouped in packs, writes a small Claude Code setup into a repository (or `~/.claude`), and answers that setup's hooks. All of its machinery lives in the binary and in a per-user cache; the repository holds only what people review.

1. **Two readers, two kinds of output.** Every output Forge produces is typed by its reader.
   - A **Finding** is for the coding agent: a problem in the change it is making, which it can fix. Hook output, Stop block reasons and deny or ask reasons are made of findings only. Its text is `path:line message`; advice starts with `Advice: `; it never carries a rule id.
   - A **Note** is for the person: a missing tool, a rule that is not enforced here, a tool version that cannot do what a rule asks, the environment, or a failure of Forge itself. Notes go to `forge apply`, `forge status` and `forge check`, and are kept in the per-user state for `forge status`. They never reach the agent.
2. **Forge's own failures never block.** An engine that did not start, a helper that broke, a time budget that ran out, an unreadable lock: each is a Note. It never fails a check, never denies a command and never blocks Stop. A hook that cannot do its work exits 0 and prints nothing.
3. **Hooks are cheap.** Every hook runs on the agent's path. Each has a budget (§8.11); only the first edit of a session may exceed it while tools warm up. Hooks on the per-edit and per-command path never download; SessionStart downloads only in detached background processes, and Stop may install a pinned tool a check needs (§17.5).
4. **No matching on prose.** Rules match code structure (ast-grep), parsed commands (the guard parses Bash into simple commands, §9) or scoped configuration files (grep with a file scope). A word in a comment, a commit message, a heredoc body or a quoted argument does not trigger a rule. The pack lints flag grep targets that read every file or reach Markdown and docs, and the built-in packs must have no lint findings (§6.6).
5. **Rules text is true everywhere it is shown.** The rules files (`.claude/rules/forge-*.md`) are committed and shared, so their text depends only on the repository's content, never on the machine (installed tools, versions, environments). Each context item holds wherever its pack is active, or carries its own `when` condition (§5.3). Machine state goes to `forge status`.
6. **The agent never sees Forge's internals.** No agent-facing text names Forge's files, commands, variables or state: `.forge`, `forge.yaml`, `forge.lock`, `forge apply`/`check`/`status`/`hook`, `claude-forge`, `FORGE_*` variables, cache paths, `accept:`. The pack lints flag these terms in rule descriptions, review questions, guard reasons and context items (§6.6), and the tests check every agent-facing text the hooks and the checker produce (§14). Forge enforces through hooks; it hands the agent no tools, so the agent has no reason to run Forge.
7. **Internals stay out of the repository.** The checker, the helpers, the built-in rules, the tool binaries and all runtime state are in the binary and the per-user cache (§4.3). What stays in the repository: `forge.yaml`, `forge.lock`, local packs and rules under `.forge/`, the hook and permission entries in `.claude/settings.json`, and the rules files.
8. **A neutral refusal for what remains visible.** When the agent reaches for the per-user cache or state (Read, Edit, Write, MultiEdit, NotebookEdit, Grep, Glob, or a Bash command naming them), the PreToolUse hook denies it with a neutral reason (§9.3), not a forbidding one.
9. **Packs stay YAML.** Built-in packs ship inside the binary as YAML and are parsed at run time; the binary's version pins the built-in set. Local packs and rules go through the same loader. A parsed cache speeds loading; YAML stays the format of record.
10. **The package manager follows the project's files.** Python and JavaScript package managers are facts read from the repository (§5.4). Forge provides uv where the project uses uv or has no packaging yet, and the guard denies `pip install` there (§9.2).
11. **Forge writes only what it owns.** Files the project owns (tool configs, `package.json`, `pyproject.toml`, `CLAUDE.md`, git config) are audited, never written; `forge init` creates missing tool configs once (§3.2).
12. **Rules are data.** One YAML file per rule, with examples that double as tests. Adding a rule means adding a file.
13. **Enforce in the closest native place:** the project's own tools, then ast-grep, then Claude Code permissions and sandbox, then hooks.

### Tests

- Finding and Note types: `crates/forge-core/src/lib.rs` (unit).
- No internals in agent text: `crates/forge-core/src/internals.rs` (unit); `crates/forge-cli/tests/apply.rs::rules_files_name_no_internals`; `crates/forge-cli/tests/hook.rs`; `crates/forge-hooks/tests/events.rs`; `crates/forge-guard/tests/corpus.rs`; the pack-example tests of `crates/forge-builtins-*/tests/`.
- Forge failures never block: `crates/forge-hooks/tests/events.rs`; §8 and §10 tests.
- Budgets: §8.11.
