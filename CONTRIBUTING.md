# Contributing to Forge

Forge is one Rust binary built from the Cargo workspace in `crates/`; the built-in packs are YAML in `packs/`, embedded in it. This page says where each kind of change goes. The specification is [`SPECS.md`](SPECS.md), an index of [`spec/`](spec/); a change to behaviour changes the spec and its tests together.

## Before you push

```sh
cargo fmt --all
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

`rust-toolchain.toml` pins the toolchain. What `forge apply` writes for the sample repositories in `fixtures/samples/` is pinned by `insta` snapshots (`crates/forge-cli/tests/snapshots/`): after a change that alters the output, review them with `cargo insta review` (from `cargo install cargo-insta`) and read the diff; it should show only what you meant to change.

Generated files have a test that rewrites them:

- `schemas/*.json` come from the loader's types: `UPDATE_SCHEMAS=1 cargo test -p forge-packs --test schemas`.
- The model ids in the ai-models rules come from `packs/ai-models/models.tsv`: `UPDATE_MODELS=1 cargo test -p forge-packs --test models_tsv`.

Tests that need the network or real tools are opt-in and run weekly in `.github/workflows/upstream.yml`: `FORGE_TEST_UPSTREAM=1` (newest npm/PyPI releases against the adapters), `FORGE_TEST_NETWORK=1` (real uv and pinned-tool downloads), `FORGE_TEST_INSTALL_TOOLS=1` (pack examples with the real pinned tools). Hook budgets: `FORGE_BUDGETS=1 cargo test --release -p forge-cli --test hook -- --ignored budgets`, on a quiet machine.

The docs site (`docs/`, Astro) is built with Bun: `cd docs && bun install && bun run dev`. It reads `claude-forge packs dump --json`, the commands' `--help` and `schemas/`.

## Where things live

| You want to change | Where |
|---|---|
| A rule, or a new one | `packs/<pack>/rules/<name>.yaml` (YAML only) |
| A pack, or a new one | `packs/<path>/pack.yaml` (`detect`, `context`), `rules/`, `init/` |
| The pack format, its schema, the content lints | `crates/forge-packs` (`model.rs`, `lint.rs`) |
| A file role (`@python-tests`) | `ROLES` in `crates/forge-packs/src/roles.rs` (and §15.4) |
| Project facts, activation, placement | `crates/forge-detect` |
| What gets written (rules files, settings, the lock, pre-commit) | `crates/forge-render` |
| How Bash commands are parsed and matched; a built-in guard check | `crates/forge-guard` (`checks/`, and `GuardCheck` in `forge-packs/src/model.rs`) |
| The checker: change sets, ast-grep and grep targets, stages, ratchets | `crates/forge-check` |
| How a release of a tool is invoked | `crates/forge-check/adapters/<tool>.yaml` (§16) |
| A builtin a check calls (`forge_hadolint`, `forge_ratchet`, …) | `crates/forge-builtins-{code,data,infra}`, `crates/forge-helpers` |
| The types engine | `crates/forge-engine` |
| Hook behaviour (`claude-forge hook <event>`) | `crates/forge-hooks` |
| Pinned tools and their downloads | `crates/forge-tools` (the catalog) |
| A command or its output | `crates/forge-cli` |

## Recipes

### A new rule

Add `packs/<pack>/rules/<name>.yaml` with `id: <pack>/<name>`, a one-sentence `description`, `scope`, `enforce` and `examples` (`match` and `no_match`, near misses included). Prefer, in order: a `tool` target (the project's linter already has the diagnostic) with an `ast-grep` stand-in, an `ast-grep` target, a `grep` target for text ast-grep cannot parse, a `check`, a `permission`, a `guard`. A guard matches the parsed command (`command: {name: git, sub: [push], flags_any: [--force]}`), never its raw text. The examples are its tests: `cargo test --workspace` runs them through the code that enforces each target. Agent-facing text (descriptions, context, findings) never names Forge's internals; the content lints fail if it does.

### A new pack

Add a directory with a `pack.yaml` (`id` equal to its path, `level`, `detect`, optionally `requires`, `context` and `tool_off`). A child pack is a sub-directory with its own `pack.yaml`; it is active only under an active parent. Context is a list of items, each with an optional `when` (`deps`, `files`, `facts`, `packs`), and true for every machine: it depends only on repository content. Add a sample under `fixtures/samples/` when the pack changes what a typical project gets.

### A new builtin

Implement `CheckBuiltin` (`forge-check/src/builtins/domain.rs`; a helper port implements `forge_core::builtin::Builtin`) in the crate of its concern and add it to that crate's `builtins()` list, named after the call without `forge_` (`hadolint` for `forge_hadolint`). A check's `run` then calls it by name. Its findings are for the agent (`path:line description`); anything for the person (a tool that could not run, a skip) is a note, which never blocks.

### A tool adapter, or a new release of a tool

A check asks a tool for an operation (`forge_do <tool> <operation> {files}`), never with a command line of its own. The adapter, `crates/forge-check/adapters/<tool>.yaml`, lists version ranges, each citing the changelog entry that makes it a range, with the arguments of each operation there (`null`: that range cannot), and `verified`, the newest release it was proven on. When the weekly upstream job reports a newer release, read its changelog, add a range if something changed, and raise `verified`; for an npm or PyPI tool, add it to `REGISTRY` in `crates/forge-check/tests/upstream.rs`.

## Writing specification tests

Behaviour in `spec/` is proven by Rust tests next to the code:

- Unit behaviour goes in the crate's `#[cfg(test)]` module; behaviour across crates in its `tests/*.rs` integration tests; end-to-end behaviour of the commands in `crates/forge-cli/tests/` (`apply.rs`, `hook.rs`, `check.rs`, `e2e.rs`), which run the built binary on a copy of a sample.
- A test's name is the requirement, in the present tense (`second_apply_is_up_to_date`); a `//` comment above it explains.
- Output that is long or easy to get subtly wrong is an `insta` snapshot; review changes with `cargo insta review`.
- A test that needs a tool which may be missing says so and does not count as passed: it prints why it skipped, or uses a stand-in on PATH that can log its arguments for the test to assert. A test that needs the network or a real tool is opt-in through a `FORGE_TEST_*` variable, and runs in the weekly upstream job.
- Tests do not depend on each other or on the machine: each works in its own temporary directory, with `FORGE_HOME` pointing into it.
- Timing tests are `#[ignore]` and opt-in (`FORGE_BUDGETS=1`), run in release on a quiet machine.
