# Getting started

## Requirements

- Claude Code **≥ 2.1.295** (`forge apply` checks `claude --version` and refuses older versions).
- Node ≥ 20 or Bun to run Forge itself.
- For the generated hooks: `sh`, `git`, `grep`, `sed`, `awk`, plus `bunx` or `uvx` for ast-grep. Your project's own tools (ruff, Biome, pytest…) are used where they are installed.

## 1. User level, once per machine

```sh
bunx @maximedepachtere/forge apply --user
# or: npx @maximedepachtere/forge apply --user
```

Forge shows the entries it will append to `~/.claude/settings.json` and asks for confirmation (`--yes` to skip the prompt). It never changes or removes an entry it did not add. If you already set something differently — for example `sandbox.enabled: false` — Forge leaves it and tells you.

## 2. Each project

In an existing repository:

```sh
cd my-app
bunx @maximedepachtere/forge apply
```

Forge detects the stack from `pyproject.toml`, `uv.lock`, `package.json` and `bun.lock`, prints the settings it appends to `.claude/settings.json`, writes its own files under `.forge/` and `.claude/`, and offers to create a `.git/hooks/pre-commit` when no hook manager is in use. Then it audits your tool configs and prints anything to fix:

```text
forge: applied (35 written, 0 deleted, 1 settings file(s) updated)
forge:typescript/no-explicit-any biome lint/suspicious/noExplicitAny: unsatisfied (…)
  add to the biome config: "linter": {"rules": {"suspicious": {"noExplicitAny": "error"}}}
```

Commit the result: the setup is part of the repository and works for everyone who clones it, with or without Forge.

For a **new project**, `forge init` first creates the stack's tool configs from the pack templates (`ruff.toml`, `pyrightconfig.json`, `biome.json`, `tsconfig.json`), each only if it does not exist, then runs `forge apply`.

In a repository you don't own, `forge apply --local` writes the permission entries to `.claude/settings.local.json` instead.

## 3. Check it

```sh
forge status        # packs, rules and where each is enforced, audits, drift, missing tools
forge explain sqlalchemy/no-query-api
sh .forge/check.sh --all      # what CI runs
```

Add the CI step to your pipeline:

```yaml
- run: sh .forge/check.sh --all
```

## Updating and removing

- `forge diff` shows what `forge apply` would change (exit 1 if anything differs).
- `forge apply` is idempotent. If someone edited a Forge-owned file, it stops and shows the diff; `--force` overwrites.
- `forge remove` deletes every owned file and exactly the settings entries recorded in the lock. The repository comes back to its state before `apply`.
