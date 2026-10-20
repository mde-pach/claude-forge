# Writing rules and packs

Rules are data: one YAML file per rule, with examples that double as tests. Adding a rule means adding a file. The [pack pages](/packs/) of this site are generated from the same files, so a new rule is documented the moment it is merged.

## Pack layout

```text
packs/<pack>/
├── pack.yaml
├── rules/<name>.yaml     # rule id: <pack>/<name>
├── context.md            # optional, ≤ 10 lines of facts for Claude
└── init/                 # optional, templates used only by forge init
```

```yaml
# packs/<pack>/pack.yaml
id: fastapi                 # ^[a-z][a-z0-9-]*$, equals the directory name
description: FastAPI conventions.
level: project              # or user (no detect)
requires: [python]
detect:
  files_any: [alembic.ini]
  deps_any: [{ecosystem: pypi, name: fastapi, version: ">=0.100.0"}]
```

`context.md` is copied verbatim to `.claude/rules/forge-<pack>.md`. Write facts, not instructions to enforce: enforcement is the rules' job.

## Rule file

```yaml
id: typescript/no-explicit-any        # <pack>/<file name>; stable, never reused
description: No explicit any. Use unknown with narrowing, or a Zod schema.
scope:
  language: typescript                # python | typescript | tsx | any
  files: ["src/**", "!**/*.test.*"]   # optional; "!" excludes
enforce:                              # ordered
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
deprecated:                           # optional
  replaced_by: typescript/other-rule
```

Every rule blocks. There is no severity: a rule you don't want is [disabled with a reason](/guide/configuration#forge-yaml).

## Targets

| Target | Fields | Effect |
|---|---|---|
| `tool` | `tool` (`biome`, `ruff`, `basedpyright`, `tsc`), `expects` (diagnostic code or rule name), `snippet` | [Audited](/guide/audits). A following `ast-grep` target is used when the tool is absent. |
| `ast-grep` | `rule` (ast-grep rule object), `mode` (`forbid`, `no-increase`, `no-decrease`), optional `constraints`, `utils` | `.forge/ast-grep/rules/<id>.yml`. `forbid`: any match in a changed file fails. `no-increase` / `no-decrease`: compared with the same file at `HEAD`. One per rule; needs a concrete language. |
| `permission` | `decision` (`deny`, `ask`), `rule` (`Bash(prefix:*)`, `Read(glob)`, `Edit(glob)`…) | Appended to `permissions.<decision>`. Prefer this to a guard whenever a prefix can express the command. |
| `guard` | `decision`, `pattern` (POSIX ERE against the JSON-escaped command) | A line in `guard.rules`. Use `[[:space:]]`, never `\s` or `\b`. |
| `sandbox` | `path`, `value` | `sandbox.<path>` in `~/.claude/settings.json` (user level). |
| `check` | `stage` (`format`, `lint`, `typecheck`, `migration`, `test`), `run`, optional `changed_run` (with `{files}`), `when_changed`, `requires_local_db`, `timeout` | A step of [`check.sh`](/guide/checks). Tool names in the commands (`ruff`, `pytest`, `prisma`…) resolve to the project's own binaries. |

## Examples are tests

`forge test` validates every pack and rule against the JSON Schemas (shipped in `schemas/`) and runs every example:

- **ast-grep** targets compile to ast-grep rule tests (`match` → invalid, `no_match` → valid) and must pass `ast-grep test`;
- **guard** examples run through `guard.sh`, each rule in isolation, under `sh`, `dash`, `bash` and BusyBox: `match` must get the rule's decision, `no_match` nothing;
- **permission** examples (`Bash(git clean -fd)`, `Read(.env)`) must match one of the rule's permission strings, and `no_match` none;
- **tool** examples feed the [audit](/guide/audits);
- every `context.md` must be ≤ 10 lines.

```sh
forge test
# forge: 15 packs, 59 rules; ast-grep 21, guard 14, permission 6, context 9: all passed
```

Run inside a repository, `forge test` also tests its `.forge/rules/`.

## Contributing a pack

1. Add `packs/<pack>/pack.yaml` and one file per rule.
2. Give every rule `match` and `no_match` examples, including the near misses you want to keep allowed.
3. Run `bun run test` — it runs `forge test`, the generator snapshots and the guard corpus. Update snapshots with `FORGE_UPDATE_SNAPSHOTS=1` when the change is intended.
4. Preview the docs with `bun run docs:dev`: the pack page appears in the sidebar and updates as you edit the YAML.
