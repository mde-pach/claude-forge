## 14. Acceptance tests

`cargo test --workspace` is the acceptance suite. It is hermetic: no network, and no tool from the machine unless a test says it needs one and skips with a printed reason when it is absent. `SPECS.md` maps each section to the test files that cover it; each chapter ends with its own list.

### 14.1 Pack examples

Every rule's `examples` run as tests, by target kind, so the built-in packs are their own golden suite. The machinery is `forge_guard::examples` and `forge_check::examples`, which `forge test` (§3.2) runs on a repository's local rules:

- ast-grep and grep targets: every `match` example matches and every `no_match` example does not (`crates/forge-check/tests/pack_examples.rs`). A rule with a `where` (§15.1) proves its `match` examples here; the types engine tests run the rest.
- guard targets: a `match` example gets one of the rule's decisions, a `no_match` example gets none, run in a directory holding the example's `files` (`crates/forge-guard/tests/examples.rs`).
- check targets: a `match` example fails its check (or gives its advice), a `no_match` example passes (`crates/forge-cli/tests/check.rs::check_examples`, which runs `forge test --builtin --json`, so every target kind's examples run there too); the domain builtins' examples run through the builtin itself (`crates/forge-builtins-code/tests/pack_examples.rs`, `crates/forge-builtins-data/tests/examples.rs`, `crates/forge-builtins-infra/tests/examples.rs`).
- An example whose tool is not installed is counted as not run, from the run's structured notes, never as a pass.
- Opt-in variables widen what runs: `FORGE_TEST_INSTALL_TOOLS=1` installs the pinned tools into Forge's cache before the code builtins' examples; `FORGE_TEST_TOOLS` picks fake tools (the default) or real ones for the infra builtins; `FORGE_TEST_NETWORK=1` runs the real-download tests of `crates/forge-tools/tests/network.rs`.
- The built-in packs have no content-lint findings (`crates/forge-packs/tests/lints.rs`) and every builtin a check command calls is registered (`crates/forge-cli/src/check.rs::every_builtin_the_packs_call_is_registered`).

### 14.2 Generator samples

`fixtures/samples/` holds six sample repositories: `fastapi-app`, `nextjs-prisma-app`, `astro-app`, `eslint-app`, `docker-app`, `pnpm-workspace-app`. For each one (`crates/forge-cli/tests/apply.rs`):

- `forge apply` output matches its golden snapshot (`crates/forge-cli/tests/snapshots/apply__<sample>.snap`): the rules files, the settings file and the lock;
- a second `apply` prints `forge: up to date` and changes nothing;
- the rules files name no Forge internals.

On a sample: an edited owned file makes `apply` stop without `--force`; `--dry-run` writes nothing; `forge remove` restores the repository; a Claude Code below the minimum is refused; `status` and `explain` report the plan; a copy of the old layout migrates (§19). End to end, `crates/forge-cli/tests/e2e.rs` runs `apply --yes`, then a violation that `forge check --changed` reports (findings apart from notes), then the fix that passes.

### 14.3 Guard corpus

`crates/forge-guard/tests/corpus.rs` loads all guard rules of all packs together and runs a corpus of commands that must ask or deny and commands that must stay silent, from a repository whose `node_modules/.bin` holds `tsc` and `vitest`. It includes the false positives of matching raw text: `git commit -m "remove chmod 777"`, `which pnpm`, `pip install --dry-run`, `gh pr create --body "…push --force…"`, and the property that quoted or heredoc text never triggers a rule. §9 lists what the corpus asserts.

### 14.4 Budgets and agent text

- Each hook's budget has a test (§8.11).
- Every agent-facing text the tests produce (rules files, findings, deny and ask reasons, Stop reasons) is checked for Forge's internals with `forge_core::internals::find` (§1, principle 6).

### 14.5 Generated spec tables

The per-pack tables of §13 and the adapter table of §16 are rendered from the loaded packs and adapter files by `crates/forge-packs/tests/spec_tables.rs` and `crates/forge-check/tests/spec_adapters.rs`. Each test fails when the files under `spec/` differ from what it renders, so the tables cannot drift; see §13 and §16 for the command that rewrites them.

### 14.6 Real Claude Code

A separate end-to-end job runs a real Claude Code ≥ 2.1.295 session against a build of this repository on a benchmark image, with the API token passed by name only. It checks that:

- the hook entries load;
- a deny reaches the agent with its reason;
- the Stop gate drives fixes;
- the transcript has no tool call into Forge's files.

It is not part of `cargo test`: it needs Claude Code and a token.

### Tests

The files named above.
