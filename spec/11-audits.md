## 11. Audits of tool targets

A `tool` target (§6.2) says that a rule is enforced by the project's own tool and config: Biome, ruff, ty, tsc or ESLint reporting a diagnostic as an error. Forge never writes that config. It audits it: it runs the project's tool on the rule's examples and reads whether the expected diagnostic is reported. Audits run in `forge apply`, `forge status` and `forge explain` (§3), at project level, never in hooks. Their results are machine state, for the person: they never reach the rules files or the agent.

### 11.1 Where a tool is configured

A rule's `tool` target is audited where its tool is configured; elsewhere the rule's `ast-grep` stand-in applies (§6.2), or `forge status` reports `not enforced here: <tool> not configured`.

| Tool | Configured by | Or depended on |
|---|---|---|
| Biome | `biome.json`, `biome.jsonc` | `@biomejs/biome` |
| ruff | `ruff.toml`, `.ruff.toml`, `[tool.ruff]` in `pyproject.toml`, or a plain-scripts project (Python sources and no `pyproject.toml`, `setup.py`, `setup.cfg` or `Pipfile`) | `ruff` |
| ty | `pyproject.toml`, `ty.toml`, or a plain-scripts project | — |
| tsc | `tsconfig.json` | `typescript` |
| ESLint | `eslint.config.{js,mjs,cjs,ts,mts,cts}` | `eslint` |

**ty's levels.** ty is Forge's own pinned tool (§17), and Forge runs it: the `typecheck` check (`forge_do ty …`) and the types engine's servers (§15). Where the project leaves ty to its defaults (no `ty.toml` and no `[tool.ty]` table in the `pyproject.toml` ty reads, looking up from the directory to the repository root), Forge sets the levels the active `tool: ty` targets expect: `--error <rule>` for each on `ty check`'s command line, and `rules: {<rule>: "error"}` in the servers' inline configuration. Those rules are then enforced with no config to add, and their audits run ty the same way, so they are satisfied. Where the project configures ty itself, its config is the authority: Forge adds no level, and an unsatisfied audit suggests the config (written for `ty.toml` without the `tool.ty.` prefix when that is the file).

In a monorepo, a tool's audits run in each workspace package that configures it or depends on it, and at the root when the root configures it (or only depends on it while no package has its config file). A target switched off by an active pack (`tool_off`, §5) is not audited: `forge status` reports `not enforced here: <tool> <expects> switched off by <pack> (<reason>)`.

### 11.2 Running an audit

For each audited target, where its tool is configured:

1. **Find the tool.** The project's own install (`node_modules/.bin`, `.venv/bin`, the project's Python environment); ty is Forge's pinned ty (§17). Not found → **skipped**: `<tool> is configured but not installed (node_modules/.bin, .venv/bin)` (`or the project's Python environment` for Python tools).
2. **Ask its adapter** (§16) for the installed version's audit operation: ruff `lint-json-stdin`, Biome and ESLint `lint-json`, tsc `typecheck`, ty `typecheck-gitlab`. A release no range covers → **unavailable**: `no Forge adapter covers <tool> <version> (Forge drives <tool> <first> and later)`. A range that cannot do it → **unavailable**: `<tool> <version> cannot <op> (it needs <tool> <version> or later)`.
3. **Check the code exists in that release.** For a tool with a code history in its adapter (ruff), a code newer than the installed release → **unavailable**: `<code> needs <tool> ≥ <since>; this project has <version>`. An unavailable audit never prints a snippet.
4. **Run each example.** A rule with no `match` example is **skipped**. Each example is written inside the rule's scope, next to the project's config, under a name no other audit uses, and removed afterwards (an example given as a one-file fixture keeps that path's directory and file type). ruff reads it from stdin with `--stdin-filename`; tsc runs with a temporary tsconfig that extends the project's and lists only the example.
5. **Read the result** from the tool's machine-readable output:
   - ruff: the JSON list's `code`; ruff has no warning level.
   - Biome: `diagnostics[].category` and `severity`.
   - ESLint: `messages[].ruleId` and `severity` (2 is an error, 1 a warning); a message with `fatal` (the example could not be parsed) skips the audit with ESLint's message.
   - ty: the GitLab report's `check_name` and `severity` (`major`, `critical`, `blocker` are errors; `minor` is a warning).
   - tsc: `error <code>` in its output.
   Output that cannot be read → **skipped**: `could not read <tool>'s output: <its first line>`.

### 11.3 Verdicts

- Every `match` example gets the expected diagnostic as an error, and no `no_match` example gets it → **satisfied**. A stricter config satisfies the rule.
- A `match` example without it → **unsatisfied**: `<expects> not reported for: <example>`.
- A `match` example that gets it only as a warning → **unsatisfied**: `<expects> is only a warning in your <tool> config, and Forge's checks do not block on warnings (e.g. <example>)`. A warning never enforces a rule.
- A `no_match` example that gets it → **unsatisfied**: `<expects> reported for a no_match example: <example>`.

Examples longer than 60 characters are cut to 57 and `...` in these texts.

### 11.4 What the person reads

Each audit prints as:

```
forge: <rule-id> <tool> <expects>[ (in <dir>)]: satisfied | unsatisfied | skipped | unavailable[ (<detail>)]
…
forge: add to the <tool> config[ (in <dir>)]:     # once per tool and directory, after the audits
  <the unsatisfied audits' snippets, merged>
```

The snippets of one tool and directory are merged into one block: TOML snippets (`[table] key = value`, or a `[table]` line and its keys) grouped under each table once, the arrays of a key joined (`extend-select = ["S110", "S112", "TID251"]`); other snippets once each, as they are.

- `forge apply` prints the unsatisfied audits.
- `forge status` prints every audit under `Audit:`, and `--json` gives them as `audits` (`rule`, `tool`, `expects`, `status`, `dir`, `detail`, `snippet`).
- `forge explain <id>` prints the rule's audits.

A rule is enforced by its tool when every place's audit of that tool is satisfied. In `forge status`'s count, an unsatisfied audit gives the reason `ruff codes not selected`, `Biome rules not at error`, `ty rules not at error`, `tsc options not set` or `ESLint rules not at error`; a skipped one `<tool> not installed` or `<tool> audit skipped`; an unavailable one its detail. The summary line is `<n> active, <m> enforced, <k> not enforced (<count> <reason>, …)`, each rule counted under its first reason, the most frequent first.

### Tests

- §11.1 where a tool is configured: `crates/forge-detect/src/place.rs` (no dedicated test of `tool_places`: covered through the plans of `crates/forge-cli/tests/apply.rs` and `crates/forge-cli/tests/e2e.rs`).
- §11.2 and §11.3: `crates/forge-check/tests/spec_audits.rs` (`ruff_satisfied_unsatisfied_and_noisy`, `the_installed_release_decides_first`, `a_tool_not_installed_is_skipped`, `a_warning_is_unsatisfied`, `ty_without_a_project_config_runs_at_the_rules_levels`), `crates/forge-cli/tests/check.rs::ty_runs_at_the_active_rules_levels_unless_the_project_configures_it`, `crates/forge-engine/tests/engine.rs::python_engine` (the inline levels, against the real ty; skipped without uv), `crates/forge-check/src/probe.rs` (unit: example paths, verdicts).
- §11.4: `crates/forge-check/tests/spec_audits.rs::enforcement_summary_counts_reasons`, `::unsatisfied_audits_suggest_one_block`, `crates/forge-cli/tests/e2e.rs::status_reports_tools_and_pin_differences`.
- ESLint, tsc and ty audits against the real tools: no test in `crates/` (they need the tools installed).
