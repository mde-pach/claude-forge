---
title: Getting started
description: Install the user-level setup once, then apply Forge in each repository.
---

You need Claude Code **≥ 2.1.295**, and Node ≥ 20 or Bun to run Forge itself. The generated hooks need `sh`, `git`, `grep`, `sed`, `awk`, and `bunx` or `uvx` for ast-grep.

## Once per machine

```sh
bunx @maximedepachtere/forge apply --user
```

Forge shows what it appends to `~/.claude/settings.json` and asks before writing. It never changes or removes a setting it did not add — see [7 · User level](../../spec/07-user-level/).

## In each repository

```sh
cd my-app
bunx @maximedepachtere/forge apply
git add .forge .claude && git commit -m "Add Forge"
```

Forge detects your stack, writes its files under `.forge/` and `.claude/`, offers a pre-commit hook, and audits your tool configs. Commit the result: everyone who clones the repository gets the setup, with or without Forge.

For a new project, `forge init` first creates the stack's tool configs (`ruff.toml`, `biome.json`, …) that are missing. In a repository you don't own, `forge apply --local` keeps the permission entries in `.claude/settings.local.json`.

## Then

```sh
forge status                    # packs, rules and where each is enforced, audits, drift
forge explain python/sqlalchemy/no-query-api
sh .forge/check.sh --all        # what CI runs
```

Add the last line as a CI step. `forge diff` shows what `apply` would change, and `forge remove` takes everything out again. The [CLI reference](../../reference/cli/) lists every command; [3 · Forge CLI](../../spec/03-cli/) specifies how they behave.
