## 1. Principles

1. **Standalone output.** Generated files never call Forge.
2. **Nothing to install.** Every hook is POSIX `sh` with `grep`, `sed`, `awk` and `git`. Project tools are the project's own binaries. Forge-only tools (ast-grep) run through pinned `bunx` / `uvx`.
3. **Forge writes only what it owns.** Files the project owns (tool configs, `package.json`, dependencies, git config, `CLAUDE.md`) are audited, never written.
4. **Rules are data.** One YAML file per rule, with examples that double as tests. Adding a rule means adding a file.
5. **Enforce in the closest native place:** the project's own tools, then ast-grep, then Claude Code permissions and sandbox, then hooks. Hooks are only for what concerns Claude's actions.
6. **Claude cannot change the setup unnoticed.** Forge-owned files are behind permission `ask` and are hash-checked by `check`. Git is the only history.
7. **Success is silent.** Every message starts with `forge:<rule-id>`.
