## 6. Rules

A rule is one YAML file: `rules/<name>.yaml` in a pack (built-in or local, §5), or `.forge/rules/<name>.yaml` in the repository. The file format is defined by the Rust model in `forge-packs` (`model.rs`); `schemas/rule.schema.json` is generated from it. Loading reads it with the strict YAML loader (§5: no anchors, aliases or tags; duplicate keys refused), validates it against the schema, then runs the checks of §6.3. A file that fails any of them refuses the whole load with its path and the reason.

### 6.1 Rule file

```yaml
id: <pack-id>/<name>           # equal to the pack's id and the file name; stable, never reused
description: string            # one line: the norm and its reason; what the agent reads when the rule fires
scope:
  language: python | typescript | tsx | yaml | any
  files: [glob]                # optional; "!" before an exclusion; @role names a file role (§15.4)
enforce: [target]              # at least one, see §6.2
examples:                      # optional, see §6.5
  match: [example]
  no_match: [example]
  deps: [{ecosystem: pypi | npm, name, version}]   # the packages the examples' project declares
review: string                 # optional, one line: the question a reviewer answers for an ask (§9)
when:                          # optional version gate
  deps: [{ecosystem, name, version}]               # every one declared, in its range
deprecated:                    # optional
  replaced_by: <rule-id>
```

- `id` is `<pack-id>/<name>`, where `<name>` is the file name without `.yaml`. A rule in a pack whose id does not match is refused.
- `description` and `review` are single lines (no newline, no tab).
- A local rule whose `id` equals a pack rule's replaces it entirely. A local rule of a pack that exists but is not active is ignored, with a warning. A local rule whose pack does not exist applies.
- `when.deps`: the rule applies only where every listed dependency is declared in its range, read as detection reads dependencies (§5): at the root for a pack that applies to the whole repository, else in the workspace packages where its pack was detected. A rule its gate leaves out is listed by `forge status` as gated, with what it needs. Each dependency in `when.deps` and `examples.deps` has a `version`.
- `deprecated`: the rule is not enforced; `forge status` lists it with its replacement.
- A rule `forge.yaml` disables (`disable: [{id, reason}]`) is not enforced; `forge status` lists it with the reason. Disabling a rule that is not active here is a warning.

**Scope.** `scope.language` picks the files a `grep` or `check` target covers and the ast-grep parser:

| `language` | Files (grep, check) | ast-grep parser |
|---|---|---|
| `python` | `*.py`, `*.pyi` | Python (`.py`, `.pyi`) |
| `typescript` | `*.ts`, `*.tsx`, `*.mts`, `*.cts`, `*.js`, `*.jsx`, `*.mjs`, `*.cjs` | Tsx (`.ts`, `.tsx`, `.mts`, `.cts`) |
| `tsx` | `*.tsx`, `*.jsx` | Tsx (`.ts`, `.tsx`, `.mts`, `.cts`) |
| `yaml` | `*.yaml`, `*.yml` | Yaml |
| `any` | every file | none: an ast-grep target needs a concrete language |

A file is in a rule's scope when it matches one of `scope.files` (or there are none), matches none of its `!` exclusions, and has one of the language's extensions. Globs are matched against repository-relative paths: `*` matches any run of characters, `/` included; `**/` matches zero or more directories; `?`, `[…]` and `{a,b}` (nesting allowed) are supported. An ast-grep target is limited by `scope.files` only; its parser decides which files it reads. For a pack detected only in some workspace packages, `scope.files` are relative to each of those packages (§5).

### 6.2 Targets

Each `enforce` entry has a `target` kind and the fields of that kind. Unknown fields are refused.

| Target | Fields | What enforces it |
|---|---|---|
| `tool` | `tool: biome \| ruff \| ty \| tsc \| eslint`, `expects` (the diagnostic: a ruff code, a Biome rule `lint/<group>/<name>`, a ty rule, a tsc option, an ESLint rule), `snippet` (the config to add when the audit is unsatisfied) | the project's own tool and config, audited (§11) |
| `ast-grep` | `mode`, `rule` (ast-grep's rule object), `constraints`, `utils` (passed to ast-grep as written), `where` (optional: engine capabilities on metavariables, §15), `needs` (optional, with `where`: packages the types engine needs installed) | the checker, with ast-grep linked in (§10) |
| `grep` | `mode`, `pattern` (a POSIX extended regular expression, one line), `ignore_case` (optional) | the checker, line by line (§10) |
| `check` | see below | the checker runs the command (§10) |
| `guard` | `decision: deny \| ask`, `command` and/or `check`, `reason` (optional) | the PreToolUse hook on Bash commands (§9) |
| `permission` | `decision: deny \| ask`, `rule` (a Claude Code permission rule, `Tool` or `Tool(specifier)`) | an entry in the settings file's `permissions.<decision>` (§7) |
| `sandbox` | `path` (a dotted key under `sandbox`), `value` | an entry in `~/.claude/settings.json` `sandbox` (§7); user level only |

`ast-grep`, `grep` and `check` targets are project level only; `sandbox` targets are user level only. A target used at the wrong level is skipped with a warning. A rule has at most one `ast-grep` target.

**Modes** (`ast-grep` and `grep`):

- `forbid`: a match on a line the change added fails; with `--all`, any match fails.
- `no-increase`: a changed file with more matches than in the base fails.
- `no-decrease`: a changed file with fewer matches than in the base fails.
- `advise`: a match on a line added during the turn is reported as advice after the edit (§10.8). Stop, the commit-time check and CI do not run it.

The agent reads a match as `<path>:<line> <description>`; an `advise` match as `<path>:<line> Advice: <description>`. The `Advice: ` prefix comes from the mode: a description does not write it.

`grep` targets are for text ast-grep does not parse: configuration, comments. Counts are matching lines per file. A grep target never reads documentation or prose files (`*.md`, `*.mdx`, `*.rst`, `*.markdown`, `*.adoc`, `docs/**`, `doc/**`, `README*`, `CHANGELOG*`, at any depth), whatever its scope.

**Tool targets and their stand-in.** A `tool` target is audited where its tool is configured (§11). An `ast-grep` target in the same rule stands in for it: it is used only when none of the rule's `tool` targets has its tool configured, and no active pack switched that diagnostic off (§5 `tool_off`). Where the tool is configured, the ast-grep target is not run.

**`check` target fields.**

| Field | Meaning |
|---|---|
| `stage` | `format`, `lint`, `codegen`, `typecheck`, `migration` or `test`: when it runs (§10.3) |
| `run` | the shell command for the whole project (`--all`, `--since`, and when no `changed_run` applies) |
| `changed_run` | optional: the command for the changed files (`{files}`), used with `--changed`, `--turn` and after an edit |
| `fix_run` | optional, `{files}` required: safe fixes applied after an edit, before the lint (§10.8) |
| `fix_lints` | optional, `true`, needs `fix_run` and `changed_run`: the fix run also reports what remains, so it is the edit's lint result and `changed_run` is not run again after the edit |
| `format_stdin` | optional, `format` and `lint` only, `{file}` required: the formatter as a filter on stdin, formatting it as `{file}` would be; used to compute added lines (§10.2) |
| `rerun` | optional, `test` only, `{files}` required: runs only the given test files, for the comparison with the start of the turn (§10.6) |
| `when_changed` | optional globs or roles: the check runs (outside `--all`) only when a changed file matches one |
| `config_files` | optional, `lint` only, globs or roles: the tool's own config files it also lints; a changed one is in the check's scope |
| `requires_local_db` | optional: the check runs only when the configured database is on this machine (§10.3) |
| `fail_on_warnings` | optional, `true`: warnings fail the check and are attributed like errors |
| `timeout` | optional, 1 to 600 seconds: the check is stopped past it and fails saying so (§10.4) |

A check command is shell. It names project tools through their adapters, `forge_do <tool> <operation> [args]` (§16): a command that runs an adapted tool directly (`ruff check {files}`, `pytest -q`) is refused with `run <tool> as "forge_do <tool> <operation>"`. A project tool whose name has a `-` and no adapter runs as `forge_exec_tool <name>`; running it directly is refused. Text in single quotes is data, not a command, for this test.

**`guard` target fields.** The format is below; how commands are parsed and decided is §9.

```yaml
- target: guard
  decision: deny | ask
  command: <match> | [<match>, …]   # any one matches
  check: ephemeral-exec | package-add | python-pm
  reason: string                    # optional, one line: replaces the description for this target
```

A guard target has `command`, `check`, or both. With both, the check decides on what `command` matched. With `check` alone, the check reads every command line. A raw regular expression on the whole command text is not accepted. A `<match>` describes one simple command; every field given must hold, and a list holds when any of its entries does (except `flags_all`):

| Field | Holds when |
|---|---|
| `name` | the command's name (its basename) is one of these; `*` is a wildcard |
| `sub` | the first positionals are these, in order; an entry is alternatives separated by `\|`, or a `/regex/` |
| `args_any` | some positional after `sub` is one of these (or matches a `/regex/` entry) |
| `last_arg` | the last positional matches this regex |
| `flags_any` | some option is one of these: `--force` (and `--force=x`, not `--force-with-lease`), `-f` (and inside a cluster such as `-uf`), `--opt=value` (or as two words) |
| `flags_all` | every one of these options is given; an entry may be alternatives separated by `\|` |
| `flags_none` | none of these options is given |
| `value_flags` | (no condition) options that take the next word as their value, besides those known for the command |
| `arg_regex` | some argument after the name matches this regex |
| `stdin` | with `arg_regex`: the regex also searches a heredoc or here-string fed to the command |
| `words` | a regex searched in the command's words from its name, joined by single spaces (the escape hatch) |
| `option_values` | some option (key: alternatives separated by `\|`) has a value matching the regex |
| `env_any` | the command assigns one of these variables (`SKIP=x git commit`, `export HUSKY=0`); `NAME=value` requires that value |
| `wrapped_by` | the command is reached through one of these wrappers (`sudo pip install`) |
| `pipe_to` | the command's output is piped into one of these commands (`*`: any) |
| `redirect_to` | the command writes, through a redirection, a file whose path matches this regex |

Names and values are matched whole, with `*` as a wildcard. The match is tried at each command position: the command itself, what a wrapper runs (`sudo`, `env`, `nice -n 5`, `timeout 5`, …) and what a runner runs (`npx`, `bunx`, `bun x`, `npm exec`, `pnpm exec|dlx`, `uv run`, `uvx`, `poetry run`, `python <script>`, `python -m <module>`, …): `name: prisma` matches `bunx prisma migrate reset`.

The built-in checks: `ephemeral-exec` (a package launcher that would fetch what it runs from a registry), `package-add` (a command that adds a named package; read-only and lock-file installs pass), `python-pm` (pip into the project's environment where the project's packages are uv's, or where it has no packaging yet). §9 says what each decides.

### 6.3 What loading refuses beyond the schema

- `rerun` outside stage `test`; `format_stdin` outside `format` and `lint`; `config_files` outside `lint`; `fix_lints` without `fix_run` and `changed_run`.
- A `guard` target with neither `command` nor `check`.
- `review` on a rule with neither a guard `ask` target nor the test-edit check (`run: forge_test_edits`).
- `needs` without `where`; `where` with a mode other than `forbid` or `advise`; `where: {X: {deprecated: …}}` written other than `deprecated: true`; `deprecated` in a Python rule's `where` (only the TypeScript engine answers it, §15).
- A dependency without a `version` in `when.deps` or `examples.deps`.
- A check command that runs an adapted tool directly, or a dashed project tool outside `forge_exec_tool`.
- A file role (`@name`) that no role defines (§15.4).
- A `detect.content_any` pattern that is not a regular expression, an unknown fact or fact value in `detect.facts_any` or a context item's `when.facts`, two context items with the same `id` and different texts, and a `requires` or `when.packs` naming an unknown pack (pack checks, §5).

### 6.4 Builtins

A check command calls Forge's builtins as shell functions named `forge_<name>` (`forge_do`, `forge_helper`, `forge_ratchet`, `forge_once`, `forge_advise`, `forge_role`, `forge_hadolint`, …). The checker runs each command with `sh -c` after a preamble that:

- sets `IFS` to a newline and turns off globbing (`set -f`);
- defines each builtin as `forge_<name>() { "$FORGE_EXE" internal <name> "$@"; }`, where `FORGE_EXE` is the running Forge executable;
- defines a wrapper for each project tool the command names (`vitest() { forge_exec_tool vitest "$@"; }`; `ty`, `terraform` and `kustomize` run Forge's choice of binary), so the command gets the project's own tool;
- sets `c_id` to the rule id and `FORGE_COOLDOWN_SECONDS` to 259200 (three days).

The command is the body of a shell function, so `return` works in it. `{files}` becomes `$FORGE_FILES` (the check's files, one per line) and `{file}` becomes `"$FORGE_FILE"`. Every `forge_<name>` a shipped pack calls is a registered builtin with a shell function (a test enforces it).

`forge internal <name> [args]` runs one builtin. It reads its context from the environment the checker set: `FORGE_ROOT` (the repository), `FORGE_BASE` (a directory holding the base copy of each changed file; unset with `--all`), `FORGE_CHANGED` (a file listing the changed files; unset with `--all`), `FORGE_RULE`, `FORGE_SCOPE` (`all`, `changed` or `turn`), `FORGE_NOTES` (where notes go), `FORGE_ACCEPTED` (the rule's accepted changes, §10.5), and the checker's own: `FORGE_FILES`, `FORGE_MODE` (`changed`, `since`, `all`, `turn`), `FORGE_SNAPSHOT`, `FORGE_ADDED`, `FORGE_ROLES`, `FORGE_CHECK_RULE`, `FORGE_CHECKS`, `FORGE_ADVISED`, `FORGE_TMP`, `FORGE_WS_ROOT`.

**Output protocol of a builtin.**

- stdout: its findings, as the agent reads them: `path:line message`, one per line; a line starting with whitespace continues the previous finding.
- Notes for the person: JSON lines appended to the `FORGE_NOTES` file, never on stdout.
- Exit status: 0 when nothing fails, 1 when a finding fails the check. A builtin whose findings are all advice marks the check as advice (§10.7).
- A finding whose text equals one of the rule's accepted changes is dropped.
- A builtin that cannot run (a missing tool, a failure of Forge itself) writes a note and exits 0: Forge's own problems never fail a check. A builtin name no crate registered writes the note `not enforced: forge_<name> is not available yet in this Forge` and exits 0.

A line of a check's output that starts with `forge:` is taken as a note for the person, never as a finding.

### 6.5 Examples

An example is one of:

- a string: code, written to a file inside the rule's scope;
- a fixture `{files: {<path>: <content>}, head: {<path>: <content>}}`: `head` is committed first, then `files` are written on top as the change being checked;
- for a guard rule, a command string or `{command, files}`: the command runs from a directory holding `files` (such as `node_modules/.bin/tsc`), which is its repository root.

Built-in packs prove their rules by their examples in Forge's test suite: every `match` example triggers the rule and no `no_match` example does (ast-grep and grep examples; check examples run through `forge check` in a temporary repository; guard examples through the guard). A `tool` target's examples are what its audit runs (§11); the test suite does not run them against real tools. A rule with `where` proves only its `match` examples without the types engine. A check example whose tool or builtin is not available is reported as skipped, read from the run's notes, never from its text.

### 6.6 Content lints

Loading never fails on these; `forge packs lint` prints them and exits 1 when there are any, and the built-in packs' tests require none. The agent-facing text of a rule is its `description`, `review` and guard `reason`s; of a pack, its `context` items' `text`.

| Lint | Finds |
|---|---|
| `cut-by-comment` | an unquoted agent-facing scalar followed by ` #` on its last line: YAML read the rest as a comment |
| `grep-scope` | a grep target (whose pattern spells letters) with no `scope.files` and language `any`, or whose `scope.files` reach Markdown or docs |
| `forge-internals` | agent-facing text naming Forge or its internals: `.forge`, `check.sh`, `forge.yaml`, `forge.lock`, `forge apply\|diff\|check\|status\|hook`, `claude-forge`, `FORGE_*` variables, `forge-state`, `models.tsv`, `accept:`, the word `Forge` |
| `enforcement-claim` | context text claiming a tool enforces the norm ("is rejected by", "rejects", "is linted by", …): false wherever the tool is not installed |
| `enforcement-limit` | agent-facing text about what is not checked ("is not checked", "not recognised", "not checked by") |
| `advice-prefix` | a description starting with `Advice`: the prefix comes from the mode |

### 6.7 Example

```yaml
# packs/typescript/rules/no-explicit-any.yaml
id: typescript/no-explicit-any
description: No explicit any. Use unknown with narrowing, or a Zod schema.
scope: {language: typescript}
enforce:
  - target: tool
    tool: biome
    expects: lint/suspicious/noExplicitAny
    snippet: '"linter": {"rules": {"suspicious": {"noExplicitAny": "error"}}}'
  - target: ast-grep
    mode: forbid
    rule: {any: [{pattern: "$A as any"}, {kind: predefined_type, regex: "^any$"}]}
examples:
  match: ["const x = y as any", "function f(a: any) {}"]
  no_match: ["const x: unknown = y", "const many = 1"]
```

### Tests

- §6.1 file format, strict YAML, layout, ids, local rules: `crates/forge-packs/tests/loading.rs`, `crates/forge-packs/tests/schemas.rs`, `crates/forge-packs/src/yaml.rs` (unit).
- §6.1 scope and globs: `crates/forge-check/src/scope.rs` (unit), `crates/forge-packs/src/glob.rs` (unit).
- §6.2 targets, modes, prose files, tool stand-in: `crates/forge-check/tests/spec_check.rs`, `crates/forge-cli/tests/check.rs`, `crates/forge-check/tests/pack_examples.rs`; guard format: `crates/forge-packs/tests/schemas.rs::guard_targets_are_structural`, `crates/forge-guard/tests/corpus.rs`.
- §6.3 refusals: `crates/forge-check/tests/spec_rules.rs`, `crates/forge-packs/tests/loading.rs::target_kind_checks`, `crates/forge-packs/src/validate.rs` (unit).
- §6.4 builtins: `crates/forge-cli/src/check.rs` (unit: every builtin the packs call is registered), `crates/forge-check/src/shell.rs` (unit), `crates/forge-check/src/builtins/mod.rs` (unit: an unregistered builtin is a note), `crates/forge-check/tests/spec_check.rs::builtin_protocol_accepts_and_notes`.
- §6.5 examples: `crates/forge-check/tests/pack_examples.rs`, `crates/forge-cli/tests/check.rs::check_examples`, `crates/forge-guard/tests/examples.rs`, the domain crates' `tests/examples.rs` and `tests/pack_examples.rs`.
- §6.6 lints: `crates/forge-packs/tests/lints.rs`, `crates/forge-packs/tests/loading.rs::lints_on_a_local_pack`, `crates/forge-packs/src/lint.rs` (unit), `crates/forge-core/src/internals.rs` (unit).
- The tool-target stand-in (ast-grep used only where the tool is not configured): no dedicated test in `crates/` (covered indirectly by the e2e samples in `crates/forge-cli/tests/e2e.rs`).
