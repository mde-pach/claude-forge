## 3. Forge CLI

- TypeScript, ESM, bundled with `bun build --target=node` into `dist/forge.js` (`#!/usr/bin/env node`). Runs on Node ≥ 20 and Bun. No Bun-only APIs.
- Published as `@<npm-scope>/forge`; binary `forge`. Run with `bunx` or `npx`.
- Packs ship inside the package. The build loads and validates them and writes the result to `dist/packs.json`, which the CLI reads (a pack or rule file that fails validation fails the build).
- Bundled libraries: YAML parser rejecting duplicate keys, anchors, aliases and custom tags; JSON Schema 2020-12 validator; JSONC reader/writer preserving comments (for `settings.json`).

### 3.1 Commands

| Command | Behavior |
|---|---|
| `forge init` | New project only. Creates the stack's tool configs from pack templates (§5.2), each only if the file does not exist (with the diagnostics active packs switch off set to off, §5.1 `tool_off`), then runs `forge apply`. |
| `forge apply` | Generate or update the project-level setup in the current repository. |
| `forge apply --user` | Generate or update the user-level setup. |
| `forge apply --local` | Like `apply`, but permission entries go to `.claude/settings.local.json` (for repositories you don't own). |
| `forge diff [--user]` | Show what `apply` would change. Exit 1 if anything differs. |
| `forge status [--user] [--json]` | Active packs; rules as `Rules: <n> active, <m> enforced, <k> not enforced (<count> <reason>, …)` then each rule with where it is enforced; disabled rules with reasons; audit results with snippets; drift; missing tools. A rule is enforced when at least one of its targets acts here: a satisfied audit, an ast-grep rule, a permission, guard, sandbox or grep target, or a check whose tools are installed. Reasons: `ruff codes not selected`, `Biome rules not at error`, `ty rules not at error`, `tsc options not set`, `ESLint rules not at error` (unsatisfied audits), `<tool> not configured`, `<tool> not installed`, `codes <tool> <version> does not have` (§16.3). Missing tools are what the generated scripts' `forge_missing_tools` prints (the SessionStart hook reports the same list). Where ty runs (§15.2), `Untyped packages: <n> installed without typing, checked as Any by ty (<the first 8 names>, … <k> more); to infer them from their source: forge.yaml python: {untyped-packages: source}` (or `none installed`, or the opt-out in effect). `Tool versions:` what the installed releases of the tools the checks drive cannot do (§16.3, `forge_tool_report`, as SessionStart prints it). `--json` prints all of it as one object: `counts`, `rules` (`id`, `enforced`, `by`, `not_enforced`), `audits`, `snippets` (§11), `missing_tools`, `untyped_packages` (`setting`, `count`, `names`), `tool_versions`, `packs`, `disabled`, `gated` (§6.1), `switched_off` (§5.1), `drift`. |
| `forge explain <rule-id>` | The rule file, where it is enforced in this project, and its audit result. |
| `forge test` | Inside a git repository: validate the project's local rules (`.forge/rules/`; a local rule replacing a pack rule is tested as written) against the schema and run their examples (§14.1). No local rules: prints that there is nothing to test, exit 0. The bundled packs are not run: they are proved by Forge's own test suite. |
| `forge remove [--user]` | Delete every owned file and exactly the settings entries recorded in the lock. |

- Every writing command accepts `--dry-run` (prints the `diff` output, writes nothing).
- `apply` is idempotent. If an owned file's hash differs from the lock, `apply` stops and prints the diff unless `--force` is given.
- Writes to a shared settings file are shown as a diff and require `y` confirmation (`--yes` for non-interactive use).
- `apply --pre-commit` consents to adding the commit-time check to a hook manager's config (§10.3); `--yes` does not, because that file is the team's and usually tracked (a script or CI job running `apply --yes` must not edit it).
