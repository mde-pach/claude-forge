## 6. Rules

### 6.1 Rule file

```yaml
id: <pack-id>/<kebab-name>     # stable, never reused
description: string            # one sentence
scope:
  language: python | typescript | tsx | yaml | any
  files: [glob]                # default: all files of the language
enforce:                       # ordered; see §6.2
  - target: <target>
    ...fields
examples:
  match: [example]             # MUST trigger the rule
  no_match: [example]          # MUST NOT trigger the rule
  deps: [{ecosystem, name, version}]  # rules with a where (§15.1): the packages, exact versions, the examples are type-checked against
when:                          # optional version gate
  deps: [{ecosystem, name, version}]  # every one declared, in its range (semver), where the rule applies
deprecated:                    # optional
  replaced_by: <rule-id>
review: string                 # optional, one line: for an ask, the one question a reviewer answers (§9.3)
```

An example is a string (code, written to a file inside the rule's scope), for a rule whose targets are all `tool`, one file `{files: {<path>: <code>}}` (the code written with that path's file type, §11), or, for rules whose targets are all `check`, a **fixture**:

```yaml
- head: {<path>: <content>}    # optional: committed first
  files: {<path>: <content>}   # then written on top: the change being checked
```

A `guard` example is a command string or `{command, files}`: the command runs from a directory holding `files` (e.g. `node_modules/.bin/tsc`), which is its repository root.

A rule with `when` is active only where each listed dependency is declared in its version range, read as detection reads it (§5.1; `package.json` `packageManager` and `devEngines.packageManager` declare the package manager as a dependency at that version): at the root for a pack that applies to the whole repository, else in the workspace packages where its pack was detected; a pack added in `forge.yaml` or required takes the packages where the gate holds when the root does not satisfy it. Its ast-grep and grep rules are scoped to those packages. A rule its gate leaves out is listed by `forge status` under `Inactive (version gate):` with what it needs (`--json`: `gated`).

Paths are relative, without `..`. Every rule has examples, except a `check` target that runs the project's tests (stage `test`), its migrations (stage `migration`), `astro check`, the Angular compiler (`ngc`) or a lock-file check (`uv`, `bun`): those need a whole project, and their examples are optional.

Every rule blocks, except an `ast-grep` or `grep` target in `advise` mode (§6.2), for heuristics whose matches need judgement. A rule you don't want is disabled with a reason.

### 6.2 Targets

| Target | Fields | Category | Effect |
|---|---|---|---|
| `tool` | `tool: biome \| ruff \| ty \| tsc \| eslint`, `expects` (diagnostic code or rule name), `snippet` (config to show when unsatisfied) | audited | §11 |
| `ast-grep` | `rule` (ast-grep rule object), `mode: forbid \| no-increase \| no-decrease \| advise`, `constraints`, `utils` (passed through), `where` (optional: engine capabilities on metavariables, §15.1) | owned | `.forge/ast-grep/rules/<rule-id>.yml` |
| `permission` | `decision: deny \| ask`, `rule` (permission rule string) | appended | settings `permissions` |
| `guard` | `decision: deny \| ask`, `pattern` (POSIX ERE against the JSON-escaped command), `check` (optional, `decision: ask` only: a built-in guard check, `ephemeral-exec`, that decides when `pattern` matches) | owned | `guard.rules` (§9.2) |
| `sandbox` | `path`, `value` | appended, user only | `~/.claude/settings.json` `sandbox` |
| `check` | `stage: format \| lint \| codegen \| typecheck \| migration \| test` (`codegen`: generated code is up to date, or is generated, before the typecheck, §10.1), `run`, `changed_run` (optional, `{files}`), `fix_run` (optional, lint only, `{files}`: safe fixes applied after an edit, §8.1), `format_stdin` (optional, format or lint, `{file}`: the formatter as a filter, formatting stdin as if it were `{file}`; formats base copies for added lines, §10.1), `rerun` (optional, test only, `{files}`: runs only the given test files, §10.6), `when_changed` (optional), `requires_local_db` (optional, boolean), `fail_on_warnings` (optional, `true`: the run fails on warnings, e.g. `oxlint --deny-warnings`, and its warning diagnostics are attributed like errors, §10.1), `config_files` (optional, lint only, globs or roles: the tool's own config files it also lints, e.g. ruff's `RUF200` on `pyproject.toml`; a changed one is in the check's scope, so `{files}`, the edit-time run and the attribution of §10.1 include it) | owned | `.forge/check.sh` (§10) |
| `grep` | `pattern` (POSIX ERE, matched line by line), `mode: forbid \| no-increase \| no-decrease \| advise`, `ignore_case` (optional) | owned | `.forge/check.sh` (§10): counts are matching lines per file |

ast-grep and grep modes:
- `forbid`: a match on an added line of a changed file fails (§10.1); with `--all`, any match.
- `no-increase`: more matches than in the base in a changed file fails.
- `no-decrease`: fewer matches than in the base in a changed file fails.
- `advise`: a match on a line added during the turn is reported after the edit (PostToolUse feedback, like `forbid`); Stop, the pre-commit hook and CI do not run it.

The schema rejects fields that would do nothing: `rerun` outside stage `test`, `format_stdin` outside `format` / `lint`, `config_files` outside `lint`, `check` on a guard with `decision: deny`, and `review` on a rule with neither a guard `ask` nor the test-edit check (`run: forge_test_edits`). A project tool whose name has a `-` (`vue-tsc`) has no wrapper function, so a check runs it as `forge_exec_tool <name>`; a rule running it directly is rejected when packs and local rules load.

A `grep` target is for text that is not code ast-grep parses: comments, configuration, `.astro` files.

`scope.language` picks the ast-grep parser and the files a check covers: `python` (`*.py`, `*.pyi`), `typescript` (`.ts`, `.tsx`, `.mts`, `.cts` and the JavaScript extensions; ast-grep `Tsx`), `tsx` (`*.tsx`, `*.jsx`), `yaml` (`*.yaml`, `*.yml`; ast-grep `Yaml`). In YAML rules, a key is matched on the `key` field of a `block_mapping_pair` with a regex that allows quotes (`^["']?name["']?$`), several `has` clauses go under `all:`, and scalars are `string_scalar`, `double_quote_scalar`, `single_quote_scalar` or `integer_scalar` (quoted kinds include their quotes).

A `tool` target is followed by an `ast-grep` target when the rule must still apply in projects that use a different tool for that job. The `ast-grep` target is used only when the `tool` target's tool is absent from the project.

### 6.3 Example

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
