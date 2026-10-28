## 16. Tool adapters

Packs drive external tools, and tools change between releases: options and output formats, which rules or codes exist, codes renamed, settings moved, what an exit status means. A rule that hard-codes one command line breaks on another release, or turns the tool's usage error into a fake finding. As §15 does for questions about the code, a rule says **what** it needs from a tool; the tool's **adapter** says how the installed release does it, or that it cannot.

### 16.1 What a rule says

- A check command names an operation of a tool, never its command line: `forge_do <tool> <operation> [arguments]` (`forge_do ruff lint {files}`, `forge_do ruff format-check .`, `forge_do ty typecheck`). The arguments are the rule's own (paths, `{files}`, `{file}`), never the tool's options. A check command that runs an adapted tool directly (`ruff check …`; single-quoted text is data, not a command) is rejected when the rule is loaded, and the engine's helpers run adapted tools only through `forge_do` (a test reads `templates/engine/`). A tool used for something else than what its adapter drives says which uses are its own (`direct`: bun runs Forge's helper scripts, `bun --no-install -e`, as a runtime; its package-manager and test commands are adapted).
- A `tool` target names the code it expects (`expects: TC004`), as the reference release of the tool names it (§16.4). Which code the installed release gives the same rule, if any, is the adapter's.

### 16.2 Adapters

`src/adapters/<tool>.ts`, one per tool, rendered into the engine (`templates/engine/adapters.sh`: `FORGE_ADAPTERS`, `FORGE_ADAPTER_META`):

- **Ranges.** Consecutive version ranges (`from` inclusive, `to` exclusive, the last one open), each citing where its behaviour is documented (the tool's changelog or release notes). Each range gives, per operation, the arguments after the tool's name, or `null`: that range cannot. The caller's arguments replace the `{1}`, `{2}`… words by rank (a value an engine helper computes: a config file it wrote) and the `{args}` word (the others), else follow the adapter's words.
- **Runner.** How the engine runs the tool: the project's own install (`node_modules/.bin`, `.venv`, its Python environment: `forge_exec_tool`, the default), the executable on `PATH` (`path`: Terraform, Helm, kubectl, Docker, the package managers), or the function that runs Forge's pinned release (`forge_ty`, `forge_pinned <tool>`). A tool that is one of several (Terraform or OpenTofu) names a **pick** function printing which adapter drives the project's (`forge_tf_bin`); the rule names the first. Its version is asked with `--version` unless the adapter says otherwise (`terraform version`, `helm version --short`).
- **Usage errors.** The exit status and output line (stdout or stderr: the tool's output is kept until it exits) by which the tool says its command line is wrong (ruff and ty: exit 2 and clap's `For more information, try '--help'.`), or none when the tool has no signature distinct from its findings (said in the adapter). Such a run is never a finding: the check prints `forge:<id> skipped: <tool> <version> rejected the command (<command>): <error>` and passes; a run formatting a base copy on stdin fails instead (§10.1: the plain comparison applies).
- **Codes.** For a tool with rule codes, the operation that lists the installed release's rules (ruff: `ruff rule --all --output-format json`: code, name, preview), and the history of each code Forge's rules expect: its first stable release, and the codes and names the same rule had (ruff renamed `TCH` to `TC` in 0.8.0, merged flake8-trio into flake8-async in 0.5.0: `TRIO100` is `ASYNC100`, the former `ASYNC100` is `ASYNC210`; renamed `PYI019`'s rule). Availability is asked from the binary first: a code is available when the installed release lists a rule with one of the rule's codes and one of its names, not in preview, and the release is not older than the history's `since` (a rule may exist before it covers what Forge's rule asks of it: flake8-trio's `TRIO110` did not flag `asyncio`; ruff's `S113` covered `httpx` from 0.5.7); it is then checked under the installed release's code (`TCH004` on ruff 0.5).
- **Pinned tools.** A tool Forge pins (ty, §15.2) has one range, the pin; its version is never detected.
- **Proven releases.** `verified` is the newest release the adapter is proven on (§16.5). A newer release is driven as the newest range says, and reported (§16.3): a tool's new release does not switch a rule off, and does not pass unnoticed either.

### 16.3 Selection and reporting

- **Version.** The installed version of a project tool is asked once (`<tool> --version`, the first `x.y[.z]`) and kept as a fact (§9.4) until the binary changes (the executable, its directory, and the file and package directory it resolves to: an npm package's files keep their archive's times, the package's directory does not). Which of the codes the active rules expect the installed release has (§16.2 Codes) likewise, for that set of codes: a few lines, read back without a process (the audits ask for every code of the tool's history at once, so a repository's targets share one answer).
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

- Each adapter is run against real binaries of the releases it claims: ruff's matrix is `TOOL_VERSIONS.ruffAdapters` (0.1.4, 0.1.15, 0.2.0, 0.5.0, 0.8.0 and the newest); every other tool's is `tests/adapters/<tool>.matrix.json`, run by `tests/spec/16-2-adapter-matrix.test.ts`. Its releases are the oldest covered (when its range can do something), each boundary and the release just before it, and the newest: at most two plus two per boundary (a single range: two); a release the matrix cannot install here (no binary for the platform, an install that no longer works) is listed with why and was run by hand, and the releases between boundaries stay documented in the adapter's header comment. It installs the tool's package alone, with a peer only where an operation needs one (each matrix says why). The real releases are opt-in (`FORGE_ADAPTER_MATRIX=1`, CI's `adapter-matrix` job): a tool's releases share one content-addressed store (bun's cache hardlinked into each release's `node_modules`, uv's cache hardlinked into each venv), removed with them once the tool's releases ran, everything goes at the end unless `FORGE_ADAPTER_MATRIX_KEEP=1`, and the matrix's directory stays within `FORGE_ADAPTER_MATRIX_BUDGET_MB` (1024 by default; asserted, the peak reported). Without it, only releases already installed run; the others are listed as skipped, saying why. Each run checks: the version is read, each call of `forge_do` exits and prints as its callers read it, a usage probe is skipped, never a finding, and a release no range covers says so. Forge's pinned tools run on their pin. For ruff, each operation of each range runs without a usage error and prints the format its callers read (one line per diagnostic for `lint`, JSON for `lint-json-stdin`, the formatted file for `format-stdin`); each code of the history is available exactly from its `since` (under the code the release gives it) and reports the rule's `match` examples there.
- `scripts/ruff-rules.ts --check` (in the tests) fails when the history differs from ruff's own tables.
- A test fails when the tool's newest release (PyPI, npm) is newer than the adapter's `verified` minor: a release no adapter was proven on is flagged, so the matrix is extended and the history regenerated.

### 16.6 Tools

Every tool a rule or an engine helper runs has an adapter; none is run with a command line of its own (§16.1). Each adapter's header comment gives, for each boundary, what changed and where the tool's changelog or release notes say so, and why its first release is the oldest covered. ruff is §16.4. The table is the adapters as they are (`src/adapters/`):

| Tool | Run | Ranges | Operations | Usage exit | Proven up to |
|---|---|---|---|---|---|
| ty | pinned | 0.0.86 | `typecheck`, `typecheck-gitlab` | 2 | 0.0.86 |
| alembic | project | 1.5.0–1.9.0 (cannot: migrations-check); 1.9.0– | `heads`, `migrations-check`, `upgrade-sql` | 2 | 1.20.1 |
| astro | project | 1.0.0– | `check` | none | 7.4.2 |
| better-auth | project | 1.0.0–1.7.6 (cannot: schema-check); 1.7.6– | `schema-check` | 1 | 1.7.14 |
| biome | project | 1.1.0–1.7.0 (cannot: lint-json); 1.7.0–1.8.0; 1.8.0–1.9.0; 1.9.0– | `ci`, `fix`, `format-stdin`, `lint`, `lint-json` | 1 | 2.6.3 |
| bun | PATH | 1.0.0–1.0.16 (cannot: test-files); 1.0.16– | `lock-check`, `test-files` | 1 | 1.4.4 |
| docker | PATH | 2.0.0–2.24.7 (cannot: validate); 2.24.7–2.35.0; 2.35.0– | `validate` | 1 | 5.6.1 |
| eslint | project | 9.0.0– | `lint`, `lint-files`, `lint-json` | 2 | 10.13.0 |
| graphql-codegen | project | 2.0.0–2.11.0 (cannot: codegen-check); 2.11.0– | `codegen-check` | none | 7.4.6 |
| helm | PATH | 3.0.0– | `lint`, `template` | 1 | 4.3.1 |
| kubectl | PATH | 1.21.0– | `build` | 1 | 1.37.2 |
| kustomize | PATH | 4.0.0– | `build` | 1 | 5.8.3 |
| mikro-orm | project | 5.0.0–5.7.11 (cannot: migrations-check); 5.7.11– | `migrate`, `migrations-check` | 1 | 7.2.5 |
| ngc | project | 9.0.0– | `typecheck` | 1 | 22.2.3 |
| npm | PATH | 8.0.0–10.5.1; 10.5.1– | `lock-check` | 1 | 12.2.1 |
| oxlint | project | 1.11.2– | `lint` | 1 | 1.90.0 |
| playwright | project | 1.12.0– | `test` | 1 | 1.64.1 |
| pnpm | PATH | 8.0.0–8.6.12 (cannot: lock-check); 8.6.12– | `lock-check` | 1/2 | 12.15.0 |
| prettier | project | 2.1.0– | `format`, `format-check`, `format-stdin` | 1 | 3.9.10 |
| prisma | project | 4.0.0–7.0.0 (cannot: migrations-check); 7.0.0– | `migrations-check`, `validate` | 1 | 7.10.1 |
| pytest | project | 7.0.0– | `test`, `test-changed`, `test-files` | 4 | 9.2.0 |
| sqlfluff | project | 1.3.0–3.0.0; 3.0.0–3.0.1 (cannot: lint); 3.0.1– | `lint` | 2 | 4.4.0 |
| terraform | PATH | 1.0.0–1.3.0 (cannot: format); 1.3.0– | `format`, `format-check`, `validate` | 1 | 1.17.0 |
| tofu | PATH | 1.6.0– | `format`, `format-check`, `validate` | 1 | 1.13.2 |
| tsc | project | 4.0.2– | `typecheck`, `typecheck-incremental` | 1 | 7.0.2 |
| typeorm | project | 0.3.0– | `migrate`, `migrations-check` | 1 | 1.1.2 |
| typeorm-ts-node-esm | project | 0.3.2– | `migrate`, `migrations-check` | 1 | 1.1.2 |
| typeorm-ts-node-commonjs | project | 0.3.2– | `migrate`, `migrations-check` | 1 | 1.1.2 |
| uv | PATH | 0.3.0–0.4.5 (cannot: build-wheel); 0.4.5–0.5.8; 0.5.8– | `build-wheel`, `lock-check`, `run-python` | 2 | 0.13.6 |
| vitest | project | 0.8.4– | `test`, `test-changed`, `test-files` | 1 | 5.0.5 |
| vue-tsc | project | 1.0.0– | `typecheck`, `typecheck-build` | 1 | 3.3.13 |
| wrangler | project | 3.0.0–4.59.0 (cannot: codegen-check); 4.59.0– | `codegen-check` | 1 | 4.154.0 |
| yarn | PATH | 2.0.0–2.4.0 (cannot: lock-check); 2.4.0–3.0.0; 3.0.0– | `lock-check` | 1 | 4.18.1 |
| actionlint | pinned | 1.7.12 | `lint` | 2 | 1.7.12 |
| ast-grep | pinned | 0.45.3 | `scan`, `scan-meta` | 2 | 0.45.3 |
| editorconfig-checker | pinned | 4.0.2 | `lint` | 2 | 4.0.2 |
| gitleaks | pinned | 8.30.1 | `scan` | 126 | 8.30.1 |
| hadolint | pinned | 2.15.1 | `lint` | 1 | 2.15.1 |
| kube-linter | pinned | 0.8.3 | `lint` | 1 | 0.8.3 |
| kubeconform | pinned | 0.8.0 | `validate` | 1 | 0.8.0 |
| lockfile-lint | pinned | 5.1.0 | `lint` | 1 | 5.1.0 |
| oasdiff | pinned | 1.33.0 | `diff` | 100 | 1.33.0 |
| osv-scanner | pinned | 2.7.0 | `scan` | 127 | 2.7.0 |
| redocly | pinned | 2.62.1 | `lint` | 1 | 2.62.1 |
| spectral | pinned | 6.17.0 | `lint` | 1 | 6.17.0 |
| squawk | pinned | 2.70.0 | `lint` | 2 | 2.70.0 |
| tflint | pinned | 0.64.0 | `lint` | 1 | 0.64.0 |
| trivy | pinned | 0.75.0 | `scan` | 1 | 0.75.0 |
| zizmor | pinned | 1.30.1 | `lint` | 2 | 1.30.1 |

Where the oldest covered release is (the rest of the adapter's header comment explains each):

- **Releases a range cannot serve.** biome before 1.7 cannot `lint-json` (the audit is `unavailable`); alembic before 1.9 has no `check`; sqlfluff 3.0.0 exits like its last file on a directory (`lint .` passes with violations: not enforced on that one release); terraform before 1.3 formats one target only; Compose before 2.24.7 resolves `env_file` with `--no-interpolate`; pnpm before 8.6.12 rewrites a stale lock under `--frozen-lockfile --lockfile-only`; Yarn before 2.4 cannot skip build scripts; uv before 0.4.5 has no `build`; bun before 1.0.16 runs no test file it is given; prisma before 7 takes the migrations check's schema differently (not enforced there); mikro-orm before 5.7.11 has no `migration:check`; graphql-codegen before 2.11 ignores `--check` and rewrites the files; wrangler before 4.59 has no `types --check`; Better Auth's CLI has `check schema` from 1.7.6 (the rule's dependency gate is `auth >=1.7.13`, unchanged).
- **A different command where an older one is unsafe.** npm before 10.5.1 deletes `node_modules` on `ci --dry-run`: its lock check is `npm ls --package-lock-only --all` there.
- **No usage-error signature.** astro (unknown options are ignored, an unknown command prints help and exits 0) and graphql-codegen (unknown options ignored): a wrong command line cannot be told from a run. lockfile-lint ignores unknown options; only a bad `--type` is recognised.
- **Version.** helm (`version --short`), kubectl (`version --client`), kustomize (`version`), kube-linter (`version`), kubeconform (`-v`), typeorm (`version`) and Compose (`docker compose version`: the plugin's) are asked otherwise than `--version`; vue-tsc (its `--version` is TypeScript's), ngc (`unknown`) and graphql-codegen (`unknown` before 7.1.1) are read from their package's `package.json`. Compose's version is kept until the `docker` executable changes, not the plugin: a plugin upgraded alone is seen when the docker CLI or its facts change.
