## 16. Tool adapters

Packs drive external tools, and tools change between releases: options and output formats, which rules or codes exist, codes renamed, settings moved, what an exit status means. A rule that hard-codes one command line breaks on another release, or turns the tool's usage error into a fake finding. As §15 does for questions about the code, a rule says **what** it needs from a tool; the tool's **adapter** says how the installed release does it, or that it cannot.

### 16.1 What a rule says

- A check command names an operation of a tool, never its command line: `forge_do <tool> <operation> [arguments]` (`forge_do ruff lint {files}`, `forge_do ruff format-check .`, `forge_do ty typecheck`). The arguments are the rule's own (paths, `{files}`, `{file}`), never the tool's options. A check command that runs an adapted tool directly (`ruff check …`) is rejected when the rule is loaded.
- A `tool` target names the code it expects (`expects: TC004`), as the reference release of the tool names it (§16.4). Which code the installed release gives the same rule, if any, is the adapter's.

### 16.2 Adapters

`src/adapters/<tool>.ts`, one per tool, rendered into the engine (`templates/engine/adapters.sh`: `FORGE_ADAPTERS`, `FORGE_ADAPTER_META`):

- **Ranges.** Consecutive version ranges (`from` inclusive, `to` exclusive, the last one open), each citing where its behaviour is documented (the tool's changelog or release notes). Each range gives, per operation, the arguments after the tool's name (the caller's arguments replace an `{args}` word, else follow them), or `null`: that range cannot.
- **Usage errors.** The exit status and output line by which the tool says its command line is wrong (ruff and ty: exit 2 and clap's `For more information, try '--help'.`). Such a run is never a finding: the check prints `forge:<id> skipped: <tool> <version> rejected the command (<command>): <error>` and passes; a run formatting a base copy on stdin fails instead (§10.1: the plain comparison applies).
- **Codes.** For a tool with rule codes, the operation that lists the installed release's rules (ruff: `ruff rule --all --output-format json`: code, name, preview), and the history of each code Forge's rules expect: its first stable release, and the codes and names the same rule had (ruff renamed `TCH` to `TC` in 0.8.0, merged flake8-trio into flake8-async in 0.5.0: `TRIO100` is `ASYNC100`, the former `ASYNC100` is `ASYNC210`; renamed `PYI019`'s rule). Availability is asked from the binary first: a code is available when the installed release lists a rule with one of the rule's codes and one of its names, not in preview, and the release is not older than the history's `since` (a rule may exist before it covers what Forge's rule asks of it: flake8-trio's `TRIO110` did not flag `asyncio`; ruff's `S113` covered `httpx` from 0.5.7); it is then checked under the installed release's code (`TCH004` on ruff 0.5).
- **Pinned tools.** A tool Forge pins (ty, §15.2) has one range, the pin; its version is never detected.
- **Proven releases.** `verified` is the newest release the adapter is proven on (§16.5). A newer release is driven as the newest range says, and reported (§16.3): a tool's new release does not switch a rule off, and does not pass unnoticed either.

### 16.3 Selection and reporting

- **Version.** The installed version of a project tool is asked once (`<tool> --version`, the first `x.y[.z]`) and kept as a fact (§9.4) until the binary changes (its path, size and time). The installed release's rule table likewise.
- **Operation.** `forge_do` takes the range covering the version. A range that cannot do the operation: `forge:<id> not enforced: <tool> <version> cannot <operation> (it needs <tool> <first release that can> or later)`, and the check passes. No range covering the version (older than the first): `forge:<id> not enforced: no Forge adapter covers <tool> <version> (Forge drives <tool> <first> and later)`. These are the only ways a whole check is not enforced because of a version.
- **Codes.** Each code is reported on its own, never the rule or the tool as a whole: `forge status` lists the codes the installed release does not have, each with the release it needs (the audit of such a target is `unavailable`: `<code> needs <tool> ≥ <since>; this project has <version>`, and never prints a snippet: a code the release does not know would break the project's config); a lint check notes `forge:<id> not enforced for <n> code(s) <tool> <version> does not have: <code> needs <tool> >= <since>, …` whatever its outcome; SessionStart prints the same once (until it changes). The other codes stay enforced (F401 on ruff 0.1.5).
- **Reports.** `forge_tool_report` (SessionStart, `forge status` `Tool versions:`) prints, per tool the active checks drive: a version no adapter covers, a version newer than the adapter was proven on, an operation a check needs that the version cannot do, and the codes it does not have. `forge status --json`: `tool_versions`.
- **Notes.** `not enforced` lines are notes like `skipped` (§10.2 step 7): they never fail a check, and are sorted after the failures in a Stop block.

### 16.4 ruff

| Range | Operations | Source |
|---|---|---|
| 0.1.4 – 0.2.0 | `lint`: `check --output-format text` (the one-line format, renamed `concise` later); `settings-warnings`: cannot (the linter's settings had no `lint` section to move to) | ruff `changelogs/0.1.x.md` 0.1.4: `--output-format` for `ruff rule` (#8203); `changelogs/0.2.x.md` 0.2.0, Deprecations |
| 0.2.0 – | `lint`: `check --output-format concise`; `settings-warnings`: `check --exit-zero --no-cache` | `changelogs/0.2.x.md` 0.2.0: `concise` and `full` output formats; settings moved under `[tool.ruff.lint]` |

Every range: `lint-json` (`check --output-format json`), `lint-json-stdin`, `fix` (`check --fix --fixable I,UP --exit-zero --show-fixes`), `format-check`, `format`, `format-stdin`, `rules`. Releases before 0.1.4 cannot list their rules and are not covered. The code history (`src/adapters/ruff-rules.ts`) is generated by `scripts/ruff-rules.ts` from `ruff rule --all --output-format json` of each release of `RUFF_MATRIX` (the first and last release of every minor from 0.1.4 to the newest): a rule is the same across releases when it has the same name, or one of its codes under a name no current rule has, to a fixed point; its `since` is the first release of the matrix where it is not in preview and, selected alone with the rule's snippet, reports the `match` examples of every target expecting the code (`src/adapters/ruff-examples.ts`; ruff stabilises rules in minor releases, so the matrix's first and last release of each minor bound it). Why not ask the binary alone: an old release cannot say what a rule did not cover yet, and why not a table alone: the binary knows which code it gives a renamed rule and whether it is in preview.

### 16.5 Tests

- Each adapter is run against real binaries of the releases it claims (the test tools' version matrix, `TOOL_VERSIONS.ruffAdapters`: 0.1.4, 0.1.15, 0.2.0, 0.5.0, 0.8.0 and the newest; as TypeScript 5, 6 and 7 are for the types engine): each operation of each range runs without a usage error and prints the format its callers read (one line per diagnostic for `lint`, JSON for `lint-json-stdin`, the formatted file for `format-stdin`); each code of the history is available exactly from its `since` (under the code the release gives it) and reports the rule's `match` examples there.
- `scripts/ruff-rules.ts --check` (in the tests) fails when the history differs from ruff's own tables.
- A test fails when the tool's newest release (PyPI, npm) is newer than the adapter's `verified` minor: a release no adapter was proven on is flagged, so the matrix is extended and the history regenerated.

### 16.6 Tools

| Tool | Adapter | State |
|---|---|---|
| ruff | `src/adapters/ruff.ts` | migrated: `python/lint`, `python/format`, `python/ruff-config-current`, the ruff audits |
| ty | `src/adapters/ty.ts` (pinned) | migrated: `python/typecheck`, the ty audits |

Not migrated yet: their checks still name a command line, and their audits run one fixed invocation. Project tools: Biome, tsc (and `vue-tsc`, `ngc`, `astro check`), ESLint, oxlint, Prettier, pytest, Vitest, Playwright, Alembic, sqlfluff, Prisma, MikroORM, graphql-codegen, wrangler, better-auth, terraform / tofu, helm, kubectl / kustomize, npm / pnpm / Yarn / Bun / uv as package managers. Forge's pinned tools (one range each, like ty): ast-grep, Squawk, zizmor, hadolint, actionlint, Redocly, Spectral, lockfile-lint, tflint, Trivy, kubeconform.
