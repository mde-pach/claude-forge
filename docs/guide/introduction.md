# Introduction

Forge is a command-line generator. It reads **rules** grouped in **packs** and writes a Claude Code setup, either for your user account (`~/.claude/`) or for one git repository. Once it is written, the setup runs with plain Claude Code: no generated file calls Forge, and you can uninstall Forge without losing anything.

## What you get

At **user level**, for every project you open:

- `deny` permissions on secrets (`.env`, SSH keys, credentials) and on history-destroying git commands;
- the Bash sandbox, with network limited to package registries and GitHub;
- a `PreToolUse` tripwire that stops `rm -rf ~`, `git push --force`, `curl … | sh` and similar commands;
- terminal notifications when Claude waits for you.

At **project level**, for the stack Forge detects in the repository:

- after each edit, the file is formatted, linted and checked against the [ast-grep rules](/packs/) of your stack, and the findings go back to Claude;
- a type check runs in the background and wakes Claude on errors;
- when Claude stops, `.forge/check.sh` verifies the change: no edited applied migration, no deleted test, no new `skip`, no fewer asserts, then format, lint, typecheck, migration checks and the related tests. If it fails, Claude keeps working (up to three times, then it hands over to you);
- the same `check.sh` runs in your pre-commit hook and CI.

## Principles

1. **Standalone output.** Generated files never call Forge.
2. **Nothing to install.** Hooks are POSIX `sh` with `grep`, `sed`, `awk` and `git`. Project tools are the project's own binaries; ast-grep runs through a pinned `bunx` / `uvx`.
3. **Forge writes only what it owns.** Your tool configs, `package.json`, dependencies, git config and `CLAUDE.md` are audited, never written.
4. **Rules are data.** One YAML file per rule, with examples that are also its tests.
5. **Enforce in the closest native place:** your own tools, then ast-grep, then permissions and the sandbox, then hooks.
6. **Claude cannot change the setup unnoticed.** Forge-owned files are behind a permission prompt and hash-checked by `check.sh`. Git is the only history.
7. **Success is silent.** Every message starts with `forge:<rule-id>`.

## Supported stack

| Slot | Python | TypeScript |
|---|---|---|
| Package manager | uv | Bun |
| Framework | FastAPI | Next.js 16 (App Router) |
| Data | SQLAlchemy 2.0 async + Alembic | Prisma 7 |
| Validation | Pydantic v2 | Zod |
| Format + lint | ruff | Biome 2 |
| Typecheck | basedpyright | `tsc` |
| Tests | pytest | Vitest, Playwright |

A project that uses other tools is never broken: rules whose tool is absent are reported as *not enforced here*, and rules that have an ast-grep fallback use it. For example, in a repository that lints with ESLint instead of Biome, `typescript/no-explicit-any` runs through ast-grep.

Platforms: macOS, Linux, WSL2. Claude Code ≥ 2.1.295.
