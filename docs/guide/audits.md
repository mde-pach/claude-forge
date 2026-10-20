# Tool audits

Your linter and type checker are the best place to enforce a rule, but Forge never edits their configs. Instead, for each `tool` target of an active rule, it **tests your config's behavior** with the rule's own examples.

1. If the tool is not used by the project, the rule's ast-grep target applies when it has one; otherwise `forge status` reports *not enforced here: &lt;tool&gt; not configured*.
2. If the tool is configured, Forge writes each example to a temporary file inside the rule's scope and runs your own binary with your config:
   - `ruff check --stdin-filename …` (stdin),
   - `biome lint <file>`, `basedpyright <file>`, and `tsc --noEmit -p <temporary tsconfig extending yours>` (temporary files, removed afterwards).
3. The expected diagnostic must appear for every *triggering* example and for none of the others. Then the rule is **satisfied** — a stricter config satisfies it automatically.
4. Otherwise the rule is **unsatisfied** and Forge prints the snippet to add:

```text
forge:python/no-bare-except ruff E722: unsatisfied (E722 not reported for: try:)
  add to the ruff config: [tool.ruff.lint] extend-select = ["E722"]
```

If the tool is configured but not installed yet, the audit is **skipped** and says so; it is never reported as satisfied.

Audits run during `forge apply` and `forge status`, never in hooks. `forge explain <rule-id>` shows the audit of one rule.

## Rules with tool targets

| Rule | Tool | Expects |
|---|---|---|
| [python/no-bare-except](/packs/python#python-no-bare-except) | ruff | `E722` |
| [python/no-explicit-any](/packs/python#python-no-explicit-any) | basedpyright, ruff | `reportExplicitAny`, `ANN401` |
| [typescript/strict](/packs/typescript#typescript-strict) | tsc | `TS7006` |
| [typescript/no-explicit-any](/packs/typescript#typescript-no-explicit-any) | Biome (else ast-grep) | `lint/suspicious/noExplicitAny` |
| [typescript/no-ts-ignore](/packs/typescript#typescript-no-ts-ignore) | Biome (else ast-grep) | `lint/suspicious/noTsIgnore` |

The pack templates used by `forge init` (`ruff.toml`, `pyrightconfig.json`, `biome.json`, `tsconfig.json`) satisfy all of them.
