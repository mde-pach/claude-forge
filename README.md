# Forge

**Guardrails for Claude Code, from rule packs.**

Forge is one binary, `claude-forge` (also installed as `forge`). It detects your stack, then sets up Claude Code for it: context Claude reads, permissions, a guard on Bash commands, checks after each edit, and a check when Claude stops that sends it back to work while something is wrong. The same checks run in your pre-commit hook and in CI.

Documentation: https://mde-pach.github.io/claude-forge/ (a page per pack, showing what each rule does at each step of the [lifecycle](https://mde-pach.github.io/claude-forge/lifecycle/)).

## Install

Any one of:

```sh
curl -LsSf https://github.com/mde-pach/claude-forge/releases/latest/download/install.sh | sh
uv tool install claude-forge
npm install -g @maximedepachtere/claude-forge
cargo install --locked --git https://github.com/mde-pach/claude-forge forge-cli
```

You need Claude Code 2.1.295 or later and `git`. Forge needs no Python, Node or Bun of its own. The hook entries call `claude-forge`, so Foundry's `forge` on your PATH never runs in their place.

## Use

```sh
forge apply --user          # once per machine: permissions and the sandbox in ~/.claude
cd my-app && forge apply    # in each repository; --yes answers its questions
git add forge.lock .claude && git commit -m "Add Forge"
```

| Command | Does |
|---|---|
| `forge apply` | detects the packs, writes the rules files, the settings entries and the lock, provisions the pinned tools, audits your tool configs. `--dry-run`, `--yes`, `--force`, `--local` (settings in `.claude/settings.local.json`), `--user`, `--pre-commit` |
| `forge init` | creates the stack's missing tool configs (`ruff.toml`, `ty.toml`, `biome.json`, `tsconfig.json`), then applies |
| `forge status` | the active packs and rules, how each is enforced here, drift, missing tools, and the notes Forge kept for you. `--json` |
| `forge check` | runs the checks: `--changed` (pre-commit), `--since <ref>` (a pull request), `--all` (the default branch). Exit 0 passed, 1 failed, 3 unverified, 4 advice only |
| `forge explain <rule>` | the rule's file and where it is enforced here |
| `forge diff` | what `apply` would change |
| `forge remove` | deletes exactly what the lock records |

A repository set up by the TypeScript Forge (0.5) is migrated by `forge apply`, which lists what it deletes and asks first.

## What lives where

In the repository, all committed:

```
forge.yaml                 your configuration (optional)
forge.lock                 Forge version, active rule set, tool pins, settings entries written
.forge/packs/<id>/         local packs (optional)
.forge/rules/*.yaml        local rules (optional)
.claude/settings.json      hook entries (claude-forge hook <event>) and permission entries
.claude/rules/forge-*.md   the context Claude reads
```

Everything else is in a per-user cache: `$FORGE_HOME`, else `$XDG_CACHE_HOME/forge`, else `~/.cache/forge` (the versioned binaries, uv and the pinned tools, the parsed packs), and state under `$XDG_STATE_HOME/forge`. The installed `forge` is a launcher that runs the version `forge.lock` records. Claude is refused access to the cache and state.

## Packs

A pack is a directory of YAML, nested language, then package, then sub-package (`typescript/nextjs/react`); a child is active only when its parent is.

- `pack.yaml`: `id` (its path), `description`, `level`, `detect` (how it is detected: files, dependencies, sources, facts such as `python.pm: uv`, …), and `context`, the items Claude reads, each with an optional `when` condition.
- `rules/<name>.yaml`: one rule each, with `scope`, `enforce` (targets: `tool`, `ast-grep`, `grep`, `check`, `permission`, `guard`, `sandbox`) and `examples` (`match` and `no_match`), which are its tests.
- `init/`: templates for `forge init`.

Built-in packs are in [`packs/`](packs/) and embedded in the binary. A project adds its own in `.forge/packs/<id>/` (same layout) and `.forge/rules/*.yaml` (a local rule with a built-in rule's id replaces it). The formats are in [`schemas/`](schemas/) and on the [writing packs](https://mde-pach.github.io/claude-forge/contributing/writing-packs/) page.

To test a pack: `cargo test --workspace` (from a clone) validates every built-in pack and runs every example through the code that enforces it; `claude-forge packs lint --repo .` checks a project's local packs and rules.

## Development

```sh
cargo test --workspace       # all tests
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings
cargo build --release -p forge-cli   # target/release/forge and claude-forge
cd docs && bun install && bun run dev   # the site, built from the binary's output
```

The specification is [`SPECS.md`](SPECS.md), an index of [`spec/`](spec/). [`CONTRIBUTING.md`](CONTRIBUTING.md) says where each kind of change goes. The state of the rewrite is in [`HANDOFF.md`](HANDOFF.md).
