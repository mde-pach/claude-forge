# Forge — Specification v0.6

Forge is one installed binary (`forge`, also `claude-forge`). It reads rules grouped in packs, writes a small Claude Code setup into a repository or `~/.claude` (rules files, settings entries, a lock), and answers that setup's hooks. Its machinery lives in the binary and a per-user cache, never in the repository.

The specification is normative: each sentence states what Forge does, in the present tense. Anything not described here is out of scope. Section numbers (`§10.4`) are stable. The implementation is the Rust workspace in `crates/`; where the code and this text disagree, one of them is a bug.

Each chapter ends with a **Tests** list. The table gathers them: the test files that cover each section, relative to `crates/`. `(unit)` means `#[cfg(test)]` tests inside the named source file.

| § | Section | Tests |
|---|---|---|
| §1 | [Principles](spec/01-principles.md) | `forge-core/src/{lib,internals}.rs` (unit); the agent-text checks of `forge-cli/tests/{apply,hook,check}.rs`, `forge-hooks/tests/events.rs`, `forge-guard/tests/corpus.rs` |
| §2 | [Stack and platform](spec/02-stack-and-platform.md) | `forge-cli/tests/{apply,spec_cli}.rs` (Claude Code gate); `forge-launcher/tests/hook_shells.rs`; `forge-tools/src/{platform,catalog}.rs` (unit) |
| §3 | [Forge CLI](spec/03-forge-cli.md) | `forge-cli/tests/{apply,spec_cli,e2e,forge_test}.rs`; `forge-launcher/tests/{exec,lock_differential}.rs`; `forge-launcher/src/{lib,note,policy,lock}.rs` (unit); `forge-home/src/lib.rs` (unit) |
| §4 | [Ownership and layout](spec/04-ownership.md) | `forge-cli/tests/{apply,spec_cli}.rs`; `forge-render/src/{settings,precommit}.rs` (unit); `forge-home/src/lib.rs` (unit) |
| §5 | [Inputs](spec/05-inputs.md) | `forge-packs/tests/{loading,schemas,lints}.rs`; `forge-detect/tests/spec_inputs.rs`; `forge-detect/src/*.rs` (unit); `forge-render/tests/{context,builtin_context}.rs`; `forge-cli/tests/spec_cli.rs` |
| §6 | [Rules](spec/06-rules.md) | `forge-packs/tests/{loading,schemas,lints}.rs`; `forge-check/tests/{spec_rules,spec_check,pack_examples}.rs`; `forge-guard/tests/{corpus,examples}.rs`; `forge-cli/src/check.rs` (unit); builtins: `forge-builtins-{code,data,infra}/tests/*` |
| §7 | [Generated output](spec/07-generated-output.md) | `forge-cli/tests/{apply,spec_cli}.rs` (snapshots in `forge-cli/tests/snapshots/`); `forge-render/tests/{context,builtin_context}.rs`; `forge-render/src/{hooks,settings,precommit}.rs` (unit); `forge-launcher/tests/hook_shells.rs`; `forge-hooks/tests/spec_hooks.rs` |
| §8 | [Hooks](spec/08-hooks.md) | `forge-hooks/tests/{events,spec_hooks}.rs`; `forge-cli/tests/hook.rs` (applied sample, budgets); `forge-check/tests/change_set.rs`; `forge-hooks/src/*.rs` (unit) |
| §9 | [Guard and review](spec/09-guard-and-review.md) | `forge-guard/tests/{corpus,examples,spec_parsing}.rs`; `forge-guard/src/parse.rs` (unit); `forge-hooks/tests/{events,spec_hooks}.rs` |
| §10 | [`forge check`](spec/10-check.md) | `forge-check/tests/{spec_check,change_set}.rs`; `forge-cli/tests/{check,e2e}.rs`; `forge-helpers/tests/golden.rs`; `forge-check/src/*.rs` (unit) |
| §11 | [Audits](spec/11-audits.md) | `forge-check/tests/spec_audits.rs`; `forge-check/src/probe.rs` (unit); `forge-cli/tests/e2e.rs` |
| §12 | [Lock](spec/12-lock.md) | `forge-render/src/lock.rs` (unit); `forge-launcher/src/lock.rs` (unit); `forge-launcher/tests/lock_differential.rs`; `forge-cli/tests/{apply,spec_cli}.rs` |
| §13 | [Packs and rules](spec/13-packs-and-rules/README.md) | `forge-packs/tests/spec_tables.rs` (the tables, kept in sync); the pack-example tests of §14.1 (the rules) |
| §14 | [Acceptance tests](spec/14-acceptance-tests.md) | the suite itself |
| §15 | [Engine capabilities](spec/15-engine-capabilities.md) | `forge-engine/tests/{engine,daemon,spec_engine}.rs`; `forge-detect/tests/spec_roles.rs` |
| §16 | [Tool adapters](spec/16-tool-adapters.md) | `forge-check/tests/spec_adapters.rs` (the tables, kept in sync, and adapter behaviour); `forge-check/src/adapters.rs` (unit); `forge-cli/tests/check.rs` |
| §17 | [Tool provisioning](spec/17-tool-provisioning.md) | `forge-tools/tests/{provision,env_file,spec_pins}.rs`; `forge-tools/tests/network.rs` (opt-in); `forge-tools/src/*.rs` (unit) |
| §18 | [Distribution](spec/18-distribution.md) | `forge-tools/tests/{install_sh,npm_package,forge_fetch}.rs` |
| §19 | [Migration from the TypeScript Forge](spec/19-migration-from-typescript.md) | `forge-cli/tests/apply.rs` (`migration_*`, on `forge-cli/tests/fixtures/old-layout`) |

### Tables generated from the code

- §13, the per-pack files and the index: `FORGE_BLESS=1 cargo test -p forge-packs --test spec_tables`.
- §16.4, the adapter tables: `FORGE_BLESS=1 cargo test -p forge-check --test spec_adapters`.
- §17.2, the tool pins: `FORGE_BLESS=1 cargo test -p forge-tools --test spec_pins`.

Without `FORGE_BLESS`, each of these tests fails when the file under `spec/` differs from what it renders.

### Behaviours without a test

Each chapter marks them where they are stated. The main ones: what the real-Claude-Code job checks (§14.6, outside `cargo test`); the ESLint, tsc and ty audits against the real tools (§11); `format_stdin`, safe fixes and `fix_lints` with a real formatter (§10); the stream-json ancestor lookup of the typecheck (§8.7); the engine resync on a new prompt (§8.5).
