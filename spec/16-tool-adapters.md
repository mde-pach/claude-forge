## 16. Tool adapters

Packs drive external tools, and tools change between releases: options, output formats, rule codes, what an exit status means. A rule that hard-codes one command line breaks on another release, or turns the tool's usage error into a false finding. So a rule says **what** it needs from a tool, and the tool's **adapter** says how the installed release does it, or that it cannot.

### 16.1 What a rule says

- A check command names an operation of a tool, never its command line: `forge_do <tool> <operation> [arguments]` (`forge_do ruff lint {files}`, `forge_do ruff format-check .`). The arguments are the rule's own (paths, `{files}`, `{file}`), never the tool's options.
- A check command that runs an adapted tool itself is rejected when its pack or local rule loads: `invalid rule: run <tool> as "forge_do <tool> <operation>" (§16: its adapter knows how each release does it)`. Text inside single quotes is data and does not count. A package manager or runtime whose other uses are not adapted says which commands count as running it (`direct`): `bun` counts only as `bun install`, `add`, `remove`, `test` and the other package-manager commands, so `bun --no-install -e` stays allowed. The loader reads the adapted tools and their `direct` patterns from the adapter files themselves (16.2) when Forge is built, so adding an adapter adds its check.
- A `tool` target names the diagnostic code it expects (`expects: S307`), as the tool's current release names it (§11).

### 16.2 Adapter data files

Each adapter is one file, `crates/forge-check/adapters/<tool>.yaml`, embedded in the binary and parsed at run time with the packs' strict YAML loader (§5). Its header comment says where each version boundary is documented. Fields:

- `tool`: the tool's name, equal to the file name.
- `verified`: the newest release the adapter is proven on.
- `runner`: where the tool comes from. `project` (the default): the project's own install (`node_modules/.bin`, `.venv/bin`, the project's Python environment; in a workspace package, then the root's). `path`: the executable on `PATH` (Terraform, Helm, kubectl, Docker, the package managers). `pinned`: Forge's pinned release from its tool cache (§17).
- `pinned`: Forge's pin. The version is the pin; it is never asked of the binary.
- `version_args`: the arguments that print the version (default `--version`). The version is the first `x.y` or `x.y.z` the tool prints on stdout or stderr. `package`: read the version from that npm package's `package.json` instead, found by walking up from the executable's real path.
- `pick`: the tool is one of several, and the adapter of the project's one drives the check. Rules name the first. `opentofu`: `tofu` when the repository has a `*.tofu` file or a `.terraform.lock.hcl` naming `registry.opentofu.org/` providers (tracked or untracked, not ignored), else `terraform`. `kustomize-or-kubectl`: `kustomize` when it is on `PATH`, else `kubectl` (its embedded `kubectl kustomize`).
- `usage`: how the tool says its command line is wrong: the exit statuses and a POSIX extended regular expression matched on each line of its output (stdout and stderr, colour codes removed). `null` when the tool has no usage signature distinct from its findings.
- `direct`: how a check command that runs the tool itself is recognised (§16.1), when it is not just the tool's name.
- `ranges`: consecutive version ranges, oldest first, without gaps: `from` inclusive, `to` exclusive, the last one open. Each cites its `source` (the changelog or release notes) and gives, for every operation, the arguments after the tool's name, or `null`: this range cannot. The caller's arguments replace the words `{1}`, `{2}`, … by rank and `{args}` (the others); without `{args}` they follow the adapter's words.
- `codes`: for a tool with rule codes, the operation that lists the installed release's rules, and the history of each code Forge's rules expect: the release it is stable from (`since`), and the codes and names the same rule had. The checker reads only `since` (§16.3); the listing operation is not run.

### 16.3 Selection and notes

`forge_do <tool> <operation> [arguments]` takes the adapter (after its `pick`), finds the tool, reads its version, and takes the range covering that version. Versions compare by their first three numbers. The range's arguments run in the check's directory; once the tool exits, its stdout and stderr are passed on and its exit status is the command's.

When it cannot run the operation, `forge_do` exits 0 and leaves a note for the person (§10), tied to the rule. It is never a finding:

| Case | Note kind | Text |
|---|---|---|
| No adapter for the tool | Forge failure | `Forge has no adapter for <tool>` |
| The tool is not installed | missing tool | `skipped: <tool> is not installed` |
| Its version could not be read | missing tool | `skipped: the version of <tool> could not be read` |
| It could not start | missing tool | `skipped: <tool> could not run: <error>` |
| Its range cannot do the operation | not enforced | `not enforced: <tool> <version> cannot <operation> (it needs <tool> <first release that can> or later)` |
| No range covers its version | not enforced | `not enforced: no Forge adapter covers <tool> <version> (Forge drives <tool> <oldest covered> and later)` |
| It rejected the command line (usage error) | not enforced | `skipped: <tool> <version> rejected the command (<tool> <arguments>): <its error line>` |

- A usage error is recognised only for an operation that does not read stdin: the output of a `*-stdin` operation is never read as one.
- Notes never reach the agent. `forge check` prints them after the findings; the hooks record them for `forge status` (§8, §10).
- A release newer than `verified` is driven as the newest range says, and reported (below): a new release neither switches a rule off nor passes unnoticed.

**Versions are facts.** The version of a project or `PATH` tool is asked once per executable and kept in Forge's state (`<state>/facts/tool-versions.json`, §4) until the executable changes: its path, modification time and size, and those of the file it resolves to. The file keeps the 200 most recently used entries, each with the time it was last used (refreshed at most once an hour, so a cache hit rarely writes); a file in another format is an empty cache.

**Report for the person.** `forge apply` (`forge: tool versions (what they cannot do is not enforced):`) and `forge status` (`Tool versions:`; `--json`: `tool_versions`) print one line per finding about the installed tools the active checks run through `forge_do`. SessionStart prints nothing of it: it is the person's to fix.

- `<tool>: its version could not be read`
- `<tool> <version>: no Forge adapter covers it (Forge drives <tool> <oldest> and later): <rule ids> not enforced`
- `<tool> <version> is newer than the releases Forge's adapter was proven on (up to <verified>): driven as <verified>` (compared by major and minor version)
- `<tool> <version> cannot <operation> (it needs <tool> <release> or later): <rule id> not enforced`

**Builtins that run a tool.** A builtin that runs a tool itself (`forge_squawk`, `forge_condor`) asks the same adapters for the operation, so the cases above are notes for it too. `forge_condor` (`python/no-dead-code`) runs Forge's pinned condor from the tree's root, so that condor reads the project's `[tool.condor]` / `[tool.vulture]`, with Forge's excludes (environments, `node_modules`, nested worktrees, `.forge`) anchored at the tree and added to the project's, and `.` only when that config names no paths. It runs a second time on a copy of the base holding only `*.py`, `*.pyi` and `pyproject.toml`. A file condor cannot parse in one of the two runs but not the other is a note (`not checked`), since their findings cannot be compared. When the run also has the `python/lint` check and the adapters can run `ruff lint`, condor's unused imports are dropped: ruff's F401 reports them (on the added lines), and the agent reads one finding, not two. A method, and a class nested in a class's body (read from the source's indentation), is advice unless its name is private: a framework calls the methods its base classes declare and reads nested classes (`get_queryset`, Django's `class Meta`), which condor cannot see.

**Codes.** The audit of a `tool` target (§11) whose expected code has a history, on a project tool older than the code's `since`, is `unavailable`: `<code> needs <tool> ≥ <since>; this project has <version>`. It prints no snippet: a code the release does not know would break the project's config. The other codes of the same tool stay enforced.

### 16.4 Adapters

The tables below are generated from the data files: `crates/forge-check/tests/spec_adapters.rs` renders them and fails when this section differs. After an adapter change, regenerate them:

```sh
FORGE_BLESS=1 cargo test -p forge-check --test spec_adapters
```

"Runs" is the runner (§16.2). In the operation tables, "cannot" is a `null` operation.

<!-- generated:adapters -->
<!-- Generated from crates/forge-check/adapters/*.yaml by crates/forge-check/tests/spec_adapters.rs; do not edit. Regenerate: FORGE_BLESS=1 cargo test -p forge-check --test spec_adapters -->

| Tool | Runs | Version read from | Proven up to | Usage error | Pick |
|---|---|---|---|---|---|
| [actionlint](#actionlint) | Forge's pin `1.7.12` | the pin | `1.7.12` | exit 2, a line matching `^(flag provided but not defined: \|invalid (boolean )?value )` |  |
| [alembic](#alembic) | the project's own | `alembic --version` | `1.20.1` | exit 2, a line matching `^[^ ]+: error: (unrecognized arguments\|argument \|the following arguments are required)` |  |
| [ast-grep](#ast-grep) | Forge's pin `0.45.3` | the pin | `0.45.3` | exit 2, a line matching `^For more information, try .--help.` |  |
| [astro](#astro) | the project's own | `astro --version` | `7.4.2` | none (no signature distinct from findings) |  |
| [better-auth](#better-auth) | the project's own | `better-auth --version` | `1.7.14` | exit 1, a line matching `^error: (unknown option\|unknown command\|too many arguments\|option .* argument missing\|missing required argument)` |  |
| [biome](#biome) | the project's own | `biome --version` | `2.6.3` | exit 1, a line matching `^(flags/invalid\|Error: )` |  |
| [bun](#bun) | `PATH` | `bun --version` | `1.4.4` | exit 1, a line matching `^error: bun [a-z]+ has no .* flag` |  |
| [condor](#condor) | Forge's pin `0.3.0` | the pin | `0.3.0` | exit 2, a line matching `^condor: error: ` |  |
| [docker](#docker) | `PATH` | `docker compose version` | `5.6.1` | exit 1 or 16, a line matching `^unknown (shorthand )?flag: \|^invalid argument .+ flag\|^flag needs an argument` |  |
| [editorconfig-checker](#editorconfig-checker) | Forge's pin `4.0.2` | the pin | `4.0.2` | exit 2, a line matching `^(flag provided but not defined: \|invalid (boolean )?value )` |  |
| [eslint](#eslint) | the project's own | `eslint --version` | `10.13.0` | exit 2, a line matching `^Invalid (option .--\|value for option )` |  |
| [gitleaks](#gitleaks) | Forge's pin `8.30.1` | the pin | `8.30.1` | exit 126, a line matching `^Error: unknown (shorthand )?flag` |  |
| [graphql-codegen](#graphql-codegen) | the project's own | `@graphql-codegen/cli`'s `package.json` | `7.4.6` | none (no signature distinct from findings) |  |
| [hadolint](#hadolint) | Forge's pin `2.15.1` | the pin | `2.15.1` | exit 1, a line matching `^(Invalid (option\|argument) \|option -.*: cannot parse value )` |  |
| [helm](#helm) | `PATH` | `helm version --short` | `4.3.1` | exit 1, a line matching `^Error: (unknown (shorthand )?flag\|invalid argument .+ flag\|flag needs an argument)` |  |
| [kube-linter](#kube-linter) | Forge's pin `0.8.3` | the pin | `0.8.3` | exit 1, a line matching `^Error: (unknown (shorthand )?flag\|unknown command\|invalid argument\|invalid format)` |  |
| [kubeconform](#kubeconform) | Forge's pin `0.8.0` | the pin | `0.8.0` | exit 1, a line matching `^(flag provided but not defined: \|invalid (boolean )?value \|.outputFormat. must be )` |  |
| [kubectl](#kubectl) | `PATH` | `kubectl version --client` | `1.37.2` | exit 1, a line matching `^[Ee]rror: (unknown (shorthand )?flag\|invalid argument .+ flag\|flag needs an argument\|specify one path to kustomization)` |  |
| [kustomize](#kustomize) | `PATH` | `kustomize version` | `5.8.3` | exit 1, a line matching `^[Ee]rror: (unknown (shorthand )?flag\|invalid argument .+ flag\|flag needs an argument\|specify one path to kustomization)` | `kubectl` when `kustomize` is not on `PATH` |
| [lockfile-lint](#lockfile-lint) | Forge's pin `5.1.0` | the pin | `5.1.0` | exit 1, a line matching `^Unable to find relevant lockfile parser for type ` |  |
| [mikro-orm](#mikro-orm) | the project's own | `mikro-orm --version` | `7.2.5` | exit 1, a line matching `^Unknown arguments?: ` |  |
| [ngc](#ngc) | the project's own | `@angular/compiler-cli`'s `package.json` | `22.2.3` | exit 1, a line matching `error.* TS(5023\|5025): .*Unknown compiler option .--\|error.* TS6044: ` |  |
| [npm](#npm) | `PATH` | `npm --version` | `12.2.1` | exit 1, a line matching `^(npm (error\|ERR!) code EUNKNOWNCONFIG\|Unknown command: )` |  |
| [oasdiff](#oasdiff) | Forge's pin `1.33.0` | the pin | `1.33.0` | exit 100, a line matching `^Error: (unknown (shorthand )?flag\|unknown command\|invalid argument)` |  |
| [osv-scanner](#osv-scanner) | Forge's pin `2.7.0` | the pin | `2.7.0` | exit 127, a line matching `^Incorrect Usage: ` |  |
| [oxlint](#oxlint) | the project's own | `oxlint --version` | `1.90.0` | exit 1, a line matching `^Error: ` |  |
| [playwright](#playwright) | the project's own | `playwright --version` | `1.64.1` | exit 1, a line matching `^error: unknown option ` |  |
| [pnpm](#pnpm) | `PATH` | `pnpm --version` | `12.16.1` | exit 1 or 2, a line matching `(ERROR[^A-Za-z]{1,8}Unknown options?: \|^For more information, try .--help.)` |  |
| [prettier](#prettier) | the project's own | `prettier --version` | `3.9.10` | exit 1 or 2, a line matching `^\[error\] Invalid --[a-zA-Z-]+ value` |  |
| [prisma](#prisma) | the project's own | `prisma --version` | `7.10.1` | exit 1, a line matching `^(! )?unknown or unexpected option: ` |  |
| [pytest](#pytest) | the project's own | `pytest --version` | `9.2.0` | exit 4, a line matching `^(ERROR\|UsageError): usage: ` |  |
| [redocly](#redocly) | Forge's pin `2.62.1` | the pin | `2.62.1` | exit 1, a line matching `^(Unknown arguments?\|Invalid values?):` |  |
| [ruff](#ruff) | the project's own | `ruff --version` | `0.17.3` | exit 2, a line matching `^For more information, try .--help.` |  |
| [spectral](#spectral) | Forge's pin `6.17.0` | the pin | `6.17.0` | exit 1, a line matching `^(Unknown arguments?\|Invalid values?):` |  |
| [sqlfluff](#sqlfluff) | the project's own | `sqlfluff --version` | `4.4.0` | exit 2, a line matching `^Try .[^ ]+ [a-z-]+ (-h\|--help). for help` |  |
| [squawk](#squawk) | Forge's pin `2.70.0` | the pin | `2.70.0` | exit 2, a line matching `^For more information, try .--help.` |  |
| [terraform](#terraform) | `PATH` | `terraform --version` | `1.17.0` | exit 1, a line matching `flag provided but not defined: \|invalid ([a-z]+ )?value .+ for (flag )?-` | `tofu` where the repository uses OpenTofu |
| [tflint](#tflint) | Forge's pin `0.64.0` | the pin | `0.64.0` | exit 1, a line matching `^Failed to parse CLI options; ` |  |
| [tofu](#tofu) | `PATH` | `tofu --version` | `1.13.2` | exit 1, a line matching `flag provided but not defined: \|invalid ([a-z]+ )?value .+ for (flag )?-` |  |
| [trivy](#trivy) | Forge's pin `0.75.0` | the pin | `0.75.0` | exit 1, a line matching `FATAL[[:space:]]+Fatal error[[:space:]]+(unknown (shorthand )?flag\|unknown command\|flag error)` |  |
| [tsc](#tsc) | the project's own | `tsc --version` | `7.0.2` | exit 1, a line matching `^error TS(5023\|5025\|6044\|6046): ` |  |
| [ty](#ty) | Forge's pin `0.0.86` | the pin | `0.0.86` | exit 2, a line matching `^For more information, try .--help.` |  |
| [typeorm](#typeorm) | the project's own | `typeorm version` | `1.1.2` | exit 1, a line matching `^(Unknown arguments?\|Missing required arguments?): ` |  |
| [typeorm-ts-node-commonjs](#typeorm-ts-node-commonjs) | the project's own | `typeorm-ts-node-commonjs version` | `1.1.2` | exit 1, a line matching `^(Unknown arguments?\|Missing required arguments?): ` |  |
| [typeorm-ts-node-esm](#typeorm-ts-node-esm) | the project's own | `typeorm-ts-node-esm version` | `1.1.2` | exit 1, a line matching `^(Unknown arguments?\|Missing required arguments?): ` |  |
| [uv](#uv) | `PATH` | `uv --version` | `0.13.6` | exit 2, a line matching `^For more information, try .--help.` |  |
| [vitest](#vitest) | the project's own | `vitest --version` | `5.0.5` | exit 1, a line matching `^CACError: Unknown option ` |  |
| [vue-tsc](#vue-tsc) | the project's own | `vue-tsc`'s `package.json` | `3.3.13` | exit 1, a line matching `^error TS(5023\|5025\|5072\|5073\|6044\|6046): ` |  |
| [wrangler](#wrangler) | the project's own | `wrangler --version` | `4.155.0` | exit 1, a line matching `ERROR.*Unknown arguments?: ` |  |
| [yarn](#yarn) | `PATH` | `yarn --version` | `4.18.1` | exit 1, a line matching `(Unknown Syntax Error\|Usage Error)` |  |
| [zizmor](#zizmor) | Forge's pin `1.30.1` | the pin | `1.30.1` | exit 2, a line matching `^For more information, try .--help.` |  |

51 adapters.

#### actionlint

| Operation | 1.7.12 – |
|---|---|
| `lint` | `-shellcheck= -pyflakes= -no-color -format {{range.}}{{.Filepath}}:{{.Line}}:{{.Column}}:{{"\x20"}}{{.Kind}}:{{"\x20"}}{{.Message}}\n{{end}} -config-file` |

- 1.7.12 –: actionlint 1.7.12: https://github.com/rhysd/actionlint/releases/tag/v1.7.12; https://github.com/rhysd/actionlint/blob/v1.7.12/docs/usage.md (Exit status)

#### alembic

| Operation | 1.5.0 – 1.9.0 | 1.9.0 – |
|---|---|---|
| `heads` | `heads` | `heads` |
| `migrations-check` | cannot | `check` |
| `upgrade-sql` | `upgrade {args} --sql` | `upgrade {args} --sql` |

- 1.5.0 – 1.9.0: alembic 1.5.0 sdist alembic/config.py (--version option; not in the changelog); changelog.rst 1.9.0 #724 (alembic check)
- 1.9.0 –: alembic.sqlalchemy.org/en/latest/changelog.html 1.9.0, feature commands #724: Added new Alembic command alembic check

#### ast-grep

| Operation | 0.45.3 – |
|---|---|
| `scan` | `scan --json=stream -c` |
| `scan-meta` | `scan --json=stream --include-metadata -c` |

- 0.45.3 –: ast-grep 0.45.3: https://github.com/ast-grep/ast-grep/releases/tag/0.45.3; ast-grep scan --help

#### astro

| Operation | 1.0.0 – |
|---|---|
| `check` | `check` |

- 1.0.0 –: withastro/astro packages/astro/CHANGELOG-v1.md 1.0.0; CHANGELOG-v3.md 3.0.0 (#8188: astro check needs @astrojs/check and typescript)

#### better-auth

| Operation | 1.0.0 – 1.7.6 | 1.7.6 – |
|---|---|---|
| `schema-check` | cannot | `check schema --config {args}` |

- 1.0.0 – 1.7.6: @better-auth/cli 1.x and auth 1.5.0-1.7.5 have no `check` command; github.com/better-auth/better-auth/releases/tag/v1.7.6 (auth: Added read-only `check` and `check schema` commands, #11314)
- 1.7.6 –: github.com/better-auth/better-auth/releases/tag/v1.7.6, `auth` Features: `check` and `check schema` commands (#11314)

#### biome

| Operation | 1.1.0 – 1.7.0 | 1.7.0 – 1.8.0 | 1.8.0 – 1.9.0 | 1.9.0 – |
|---|---|---|---|---|
| `ci` | `ci` | `ci` | `ci` | `ci` |
| `fix` | `check --apply --no-errors-on-unmatched` | `check --apply --no-errors-on-unmatched` | `check --write --no-errors-on-unmatched` | `check --write --no-errors-on-unmatched` |
| `format-stdin` | `format --stdin-file-path {args}` | `format --stdin-file-path {args}` | `format --stdin-file-path {args}` | `format --stdin-file-path {args}` |
| `lint` | `lint --colors=off --max-diagnostics=200` | `lint --colors=off --max-diagnostics=200` | `lint --colors=off --max-diagnostics=200` | `lint --colors=off --max-diagnostics=none` |
| `lint-json` | cannot | `lint --reporter=json --max-diagnostics=50` | `lint --reporter=json --max-diagnostics=50` | `lint --reporter=json --max-diagnostics=50` |

- 1.1.0 – 1.7.0: CHANGELOG_v1.md 1.1.0 (`// biome-ignore`); 1.7.0 (`--reporter=json`)
- 1.7.0 – 1.8.0: CHANGELOG_v1.md 1.7.0: `--reporter=json` and `--reporter=json-pretty`
- 1.8.0 – 1.9.0: CHANGELOG_v1.md 1.8.0: new options `--write`, `--fix` and `--unsafe` (`--apply` deprecated, removed in 2.0.0)
- 1.9.0 –: CHANGELOG_v1.md v1.9.0: `--max-diagnostics` accepts `none`

#### bun

| Operation | 1.0.0 – 1.0.16 | 1.0.16 – |
|---|---|---|
| `lock-check` | `install --frozen-lockfile` | `install --frozen-lockfile` |
| `test-files` | cannot | `test` |

- 1.0.0 – 1.0.16: bun 1.0.0–1.0.15 binaries: bun test ./file runs 0 files, exit 0 (release notes silent; fixed in 1.0.16, bun.sh/blog/bun-v1.0.16)
- 1.0.16 –: bun 1.0.16 binary (bun test ./file runs it); bun.com/blog/bun-v1.4.4 (unknown flags are errors in package-manager commands)
- A check command runs it itself (§16.1) when it matches ``(^\|[;&\|`!{]\|\$\(\|\b(?:then\|do\|else\|exec\|time)\s)\s*bun\s+(?:install\|i\|add\|a\|remove\|rm\|update\|upgrade\|outdated\|link\|unlink\|pm\|test\|ci\|audit\|publish\|patch\|patch-commit\|why\|info)(?=$\|[\s;&\|)`])``.

#### condor

| Operation | 0.3.0 – |
|---|---|
| `dead-code` | `--format json` |

- 0.3.0 –: condor 0.3.0 (same JSON, kinds and exit codes as 0.2.0): condor --help; https://mde-pach.github.io/condor/ (exit codes); CHANGELOG.md

#### docker

| Operation | 2.0.0 – 2.24.7 | 2.24.7 – 2.35.0 | 2.35.0 – |
|---|---|---|---|
| `validate` | cannot | `compose config --quiet --no-interpolate` | `compose config --quiet --no-interpolate --no-env-resolution` |

- 2.0.0 – 2.24.7: github.com/docker/compose/releases/tag/v2.24.7: restore support for config --no-interpolate (#11556)
- 2.24.7 – 2.35.0: github.com/docker/compose/releases/tag/v2.24.7 (#11556); v2.35.0 (#12665: --no-env-resolution)
- 2.35.0 –: github.com/docker/compose/releases/tag/v2.35.0: Introduce config --no-env-resolution (#12665)

#### editorconfig-checker

| Operation | 4.0.2 – |
|---|---|
| `lint` | `-no-color -format gcc -config` |

- 4.0.2 –: editorconfig-checker 4.0.2: https://github.com/editorconfig-checker/editorconfig-checker/releases/tag/v4.0.2; editorconfig-checker -help

#### eslint

| Operation | 9.0.0 – |
|---|---|
| `lint` | `--max-warnings=0` |
| `lint-files` | `--max-warnings=0 --no-warn-ignored --no-error-on-unmatched-pattern` |
| `lint-json` | `--format json --no-warn-ignored` |

- 9.0.0 –: eslint CHANGELOG.md v9.0.0: flat config is default (#17748); v10.0.0: remove eslintrc support (#20037)

#### gitleaks

| Operation | 8.30.1 – |
|---|---|
| `scan` | `dir --no-banner --no-color --redact -l error --ignore-gitleaks-allow --exit-code 0 --report-format template {args}` |

- 8.30.1 –: gitleaks 8.30.1: https://github.com/gitleaks/gitleaks/releases/tag/v8.30.1; cmd/root.go (exit 126 on an unknown flag); gitleaks dir --help

#### graphql-codegen

| Operation | 2.0.0 – 2.11.0 | 2.11.0 – |
|---|---|---|
| `codegen-check` | cannot | `--check` |

- 2.0.0 – 2.11.0: packages/graphql-codegen-cli/CHANGELOG.md 2.11.0 (fd6be805b): `--check` added; before, it is ignored and the outputs are written
- 2.11.0 –: packages/graphql-codegen-cli/CHANGELOG.md 2.11.0 (fd6be805b): dry-run mode with `--check`

#### hadolint

| Operation | 2.15.1 – |
|---|---|
| `lint` | `--disable-ignore-pragma --no-fail --no-color -f tty -c` |

- 2.15.1 –: hadolint 2.15.1: https://github.com/hadolint/hadolint/releases/tag/v2.15.1; hadolint --help

#### helm

| Operation | 3.0.0 – |
|---|---|
| `lint` | `lint --strict` |
| `template` | `template forge` |

- 3.0.0 –: github.com/helm/helm/releases/tag/v3.0.0 (template NAME CHART; -n is --namespace); v4.0.0 release notes (no change to lint/template invocations, verified)

#### kube-linter

| Operation | 0.8.3 – |
|---|---|
| `lint` | `lint --format plain --with-color=false --config` |

- 0.8.3 –: kube-linter 0.8.3: https://github.com/stackrox/kube-linter/releases/tag/v0.8.3; kube-linter lint --help

#### kubeconform

| Operation | 0.8.0 – |
|---|---|
| `validate` | `-strict -output json -schema-location default -schema-location https://raw.githubusercontent.com/datreeio/CRDs-catalog/main/{{.Group}}/{{.ResourceKind}}_{{.ResourceAPIVersion}}.json -cache` |

- 0.8.0 –: kubeconform 0.8.0: https://github.com/yannh/kubeconform/releases/tag/v0.8.0; kubeconform -h

#### kubectl

| Operation | 1.21.0 – |
|---|---|
| `build` | `kustomize` |

- 1.21.0 –: kubernetes CHANGELOG/CHANGELOG-1.21.md, Kustomize Updates in Kubectl (v2.0.3 -> v4.0.5, #98946); CHANGELOG-1.14.md #73033 (kubectl kustomize)

#### kustomize

| Operation | 4.0.0 – |
|---|---|
| `build` | `build` |

- 4.0.0 –: kubernetes CHANGELOG-1.21 (Kustomize Updates in Kubectl: v2.0.3 -> v4.0.5); github.com/kubernetes-sigs/kustomize/releases/tag/kustomize%2Fv5.0.0

#### lockfile-lint

| Operation | 5.1.0 – |
|---|---|
| `lint` | `--type npm --allowed-hosts npm --validate-https --validate-integrity --path` |

- 5.1.0 –: lockfile-lint 5.1.0: https://github.com/lirantal/lockfile-lint/releases/tag/lockfile-lint%405.1.0; lockfile-lint --help

#### mikro-orm

| Operation | 5.0.0 – 5.7.11 | 5.7.11 – |
|---|---|---|
| `migrate` | `migration:up` | `migration:up` |
| `migrations-check` | cannot | `migration:check` |

- 5.0.0 – 5.7.11: CHANGELOG.md 5.7.11: cli: expose missing `migration:check` command (#4388)
- 5.7.11 –: CHANGELOG.md 5.7.11: cli: expose missing `migration:check` command (#4388)

#### ngc

| Operation | 9.0.0 – |
|---|---|
| `typecheck` | `-p {args} --noEmit` |

- 9.0.0 –: angular CHANGELOG_ARCHIVE.md 9.0.0: Angular now compiles with Ivy by default (#32219)

#### npm

| Operation | 8.0.0 – 10.5.1 | 10.5.1 – |
|---|---|---|
| `lock-check` | `ls --package-lock-only --all` | `ci --dry-run --ignore-scripts --no-audit --no-fund` |

- 8.0.0 – 10.5.1: npm CHANGELOG v8.4.1 (fix(ci): lock file validation, #4363); release/v10 CHANGELOG v10.5.1 (added check for dry-run, #7274): before 10.5.1 ci --dry-run deletes node_modules
- 10.5.1 –: release/v10 CHANGELOG v10.5.1 (added check for dry-run, #7274); CHANGELOG 12.0.0-pre.1 (#9276: unknown flags are errors)
- A check command runs it itself (§16.1) when it matches ``(^\|[;&\|`!{]\|\$\(\|\b(?:then\|do\|else\|exec\|time)\s)\s*npm(?=$\|[\s;&\|)`])``.

#### oasdiff

| Operation | 1.33.0 – |
|---|---|
| `diff` | `breaking {args} --fail-on ERR --format singleline` |

- 1.33.0 –: oasdiff 1.33.0: https://github.com/oasdiff/oasdiff/releases/tag/v1.33.0; internal/errors.go (exit codes 100-107)

#### osv-scanner

| Operation | 2.7.0 – |
|---|---|
| `scan` | `scan source --format markdown --config` |

- 2.7.0 –: osv-scanner 2.7.0: https://github.com/google/osv-scanner/releases/tag/v2.7.0; https://github.com/google/osv-scanner/blob/v2.7.0/docs/output.md (Return Codes)

#### oxlint

| Operation | 1.11.2 – |
|---|---|
| `lint` | `--type-aware --deny-warnings --format unix` |

- 1.11.2 –: oxc apps/oxlint/CHANGELOG.md 1.11.0 (#12485 `--type-aware`); 1.11.2 (#12984 error when tsgolint cannot spawn)

#### playwright

| Operation | 1.12.0 – |
|---|---|
| `test` | `test --reporter=line` |

- 1.12.0 –: github.com/microsoft/playwright/releases/tag/v1.12.0: Introducing Playwright Test (`npx playwright test`, #6816)

#### pnpm

| Operation | 8.0.0 – 8.6.12 | 8.6.12 – |
|---|---|---|
| `lock-check` | cannot | `install --frozen-lockfile --lockfile-only` |

- 8.0.0 – 8.6.12: pnpm/CHANGELOG.md 8.6.12 (#6913: --frozen-lockfile --lockfile-only fails on a stale lock); 7.4.1 (#4951)
- 8.6.12 –: pnpm/CHANGELOG.md 8.6.12 (#6913)
- A check command runs it itself (§16.1) when it matches ``(^\|[;&\|`!{]\|\$\(\|\b(?:then\|do\|else\|exec\|time)\s)\s*pnpm(?=$\|[\s;&\|)`])``.

#### prettier

| Operation | 2.1.0 – |
|---|---|
| `format` | `--write --ignore-unknown` |
| `format-check` | `--check` |
| `format-stdin` | `--stdin-filepath {args}` |

- 2.1.0 –: prettier CHANGELOG.md 2.1.0 / prettier.io/blog/2020/08/24/2.1.0.html: `--ignore-unknown` (#8829)

#### prisma

| Operation | 4.0.0 – 7.0.0 | 7.0.0 – |
|---|---|---|
| `migrations-check` | cannot | `migrate diff --from-migrations prisma/migrations --to-schema prisma/schema.prisma --exit-code` |
| `validate` | `validate` | `validate` |

- 4.0.0 – 7.0.0: github.com/prisma/prisma/releases/tag/7.0.0: Removal of `prisma migrate` flags (`--to-schema-datamodel` -> `--to-schema`); shadowDatabaseUrl moved to the config file
- 7.0.0 –: github.com/prisma/prisma/releases/tag/7.0.0: `--[from/to]-schema`; `datasource.shadowDatabaseUrl` in prisma.config.ts

#### pytest

| Operation | 7.0.0 – |
|---|---|
| `test` | `-q` |
| `test-changed` | `-q --testmon` |
| `test-files` | `-q` |

- 7.0.0 –: docs.pytest.org/en/stable/changelog.html pytest 7.0.0 (#8246: --version writes to stdout)

#### redocly

| Operation | 2.62.1 – |
|---|---|
| `lint` | `lint --format=github-actions` |

- 2.62.1 –: @redocly/cli 2.62.1: https://github.com/Redocly/redocly-cli/releases/tag/%40redocly%2Fcli%402.62.1; redocly lint --help

#### ruff

| Operation | 0.1.4 – 0.2.0 | 0.2.0 – |
|---|---|---|
| `fix` | `check --fix --fixable I,UP --show-fixes --output-format text` | `check --fix --fixable I,UP --show-fixes --output-format concise` |
| `format` | `format` | `format` |
| `format-check` | `format --check` | `format --check` |
| `format-stdin` | `format --stdin-filename {args} -` | `format --stdin-filename {args} -` |
| `lint` | `check --output-format text` | `check --output-format concise` |
| `lint-json` | `check --output-format json` | `check --output-format json` |
| `lint-json-stdin` | `check --no-cache --no-fix --output-format json --stdin-filename {args} -` | `check --no-cache --no-fix --output-format json --stdin-filename {args} -` |
| `rules` | `rule --all --output-format json` | `rule --all --output-format json` |
| `settings-warnings` | cannot | `check --exit-zero --no-cache` |

- 0.1.4 – 0.2.0: changelogs/0.1.x.md 0.1.4 (#8203); changelogs/0.2.x.md 0.2.0 Deprecations
- 0.2.0 –: changelogs/0.2.x.md 0.2.0 (concise output format; settings moved under lint)
- Codes: the history of 116 codes Forge's rules expect (§16.3); `rules` lists the release's rules.

#### spectral

| Operation | 6.17.0 – |
|---|---|
| `lint` | `lint --format text --fail-severity error --ruleset` |

- 6.17.0 –: @stoplight/spectral-cli 6.17.0: https://github.com/stoplightio/spectral/releases/tag/%40stoplight%2Fspectral-cli%406.17.0; spectral lint --help

#### sqlfluff

| Operation | 1.3.0 – 3.0.0 | 3.0.0 – 3.0.1 | 3.0.1 – |
|---|---|---|---|
| `lint` | `lint --format github-annotation-native --nocolor` | cannot | `lint --format github-annotation-native --nocolor` |

- 1.3.0 – 3.0.0: CHANGELOG.md 1.3.0: Consistent return codes (#3608)
- 3.0.0 – 3.0.1: CHANGELOG.md 3.0.1: hotfix for the CLI exit codes broken in 3.0.0 (#5676)
- 3.0.1 –: CHANGELOG.md 3.0.1 (#5676); 3.2.3 groups per file (#6312)

#### squawk

| Operation | 2.70.0 – |
|---|---|
| `lint` | `--reporter gcc -c` |

- 2.70.0 –: squawk 2.70.0: https://github.com/sbdchd/squawk/releases/tag/v2.70.0; squawk --help

#### terraform

| Operation | 1.0.0 – 1.3.0 | 1.3.0 – |
|---|---|---|
| `format` | cannot | `fmt` |
| `format-check` | `fmt -check -recursive` | `fmt -check -recursive` |
| `validate` | `validate -no-color` | `validate -no-color` |

- 1.0.0 – 1.3.0: developer.hashicorp.com/terraform/language/v1-compatibility-promises (1.x); hashicorp/terraform v1.3/CHANGELOG.md 1.3.0 (fmt accepts multiple target paths, #28191)
- 1.3.0 –: hashicorp/terraform v1.3/CHANGELOG.md 1.3.0 ENHANCEMENTS: terraform fmt now accepts multiple target paths (#28191)

#### tflint

| Operation | 0.64.0 – |
|---|---|
| `lint` | `--format compact --force --chdir` |

- 0.64.0 –: tflint 0.64.0: https://github.com/terraform-linters/tflint/releases/tag/v0.64.0; tflint --help (--force: return zero exit status even if issues found)

#### tofu

| Operation | 1.6.0 – |
|---|---|
| `format` | `fmt` |
| `format-check` | `fmt -check -recursive` |
| `validate` | `validate -no-color` |

- 1.6.0 –: github.com/opentofu/opentofu/releases/tag/v1.6.0 (first stable release); v1.6/CHANGELOG.md

#### trivy

| Operation | 0.75.0 – |
|---|---|
| `scan` | `config --quiet --misconfig-scanners terraform --severity HIGH,CRITICAL --skip-check-update --format template {args}` |

- 0.75.0 –: trivy 0.75.0: https://github.com/aquasecurity/trivy/releases/tag/v0.75.0; trivy config --help

#### tsc

| Operation | 4.0.2 – |
|---|---|
| `typecheck` | `--noEmit -p {args} --pretty false` |
| `typecheck-incremental` | `--noEmit -p tsconfig.json --pretty false --incremental --tsBuildInfoFile {args}` |

- 4.0.2 –: TypeScript 4.0 release notes: `--incremental` with `--noEmit` (typescriptlang.org/docs/handbook/release-notes/typescript-4-0.html)

#### ty

| Operation | 0.0.86 – |
|---|---|
| `typecheck` | `check --output-format concise` |
| `typecheck-gitlab` | `check --output-format gitlab` |

- 0.0.86 –: ty 0.0.86: ty check --help

#### typeorm

| Operation | 0.3.0 – |
|---|---|
| `migrate` | `migration:run -d {args}` |
| `migrations-check` | `migration:generate -d {args} --check` |

- 0.3.0 –: CHANGELOG.md 0.3.0: CLI commands take a data source file (-d); 0.2.32 (#7275): migration:generate --check

#### typeorm-ts-node-commonjs

| Operation | 0.3.2 – |
|---|---|
| `migrate` | `migration:run -d {args}` |
| `migrations-check` | `migration:generate -d {args} --check` |

- 0.3.2 –: npm typeorm@0.3.2 package.json bin (first with typeorm-ts-node-*); CHANGELOG.md 0.3.0 (-d)

#### typeorm-ts-node-esm

| Operation | 0.3.2 – |
|---|---|
| `migrate` | `migration:run -d {args}` |
| `migrations-check` | `migration:generate -d {args} --check` |

- 0.3.2 –: npm typeorm@0.3.2 package.json bin (first with typeorm-ts-node-*); CHANGELOG.md 0.3.0 (-d)

#### uv

| Operation | 0.3.0 – 0.4.5 | 0.4.5 – 0.5.8 | 0.5.8 – |
|---|---|---|---|
| `build-wheel` | cannot | `build --wheel --quiet -o` | `build --wheel --quiet -o` |
| `lock-check` | `lock --locked` | `lock --locked` | `lock --check` |
| `run-python` | `run --no-project --quiet python -I -c` | `run --no-project --quiet python -I -c` | `run --no-project --quiet python -I -c` |

- 0.3.0 – 0.4.5: uv changelogs/0.3.x.md 0.3.0 (projects stable); changelogs/0.4.x.md 0.4.5 (uv build, #6895)
- 0.4.5 – 0.5.8: uv changelogs/0.4.x.md 0.4.5 (Implement uv build, #6895; uv run reflects the exit code)
- 0.5.8 –: uv changelogs/0.5.x.md 0.5.8 (--locked/--frozen reframed as --check for uv lock); changelogs/0.8.x.md 0.8.0 (#14167: stale lock exits 1)
- A check command runs it itself (§16.1) when it matches ``(^\|[;&\|`!{]\|\$\(\|\b(?:then\|do\|else\|exec\|time)\s)\s*uv(?=$\|[\s;&\|)`])``.

#### vitest

| Operation | 0.8.4 – |
|---|---|
| `test` | `run` |
| `test-changed` | `run --changed` |
| `test-files` | `run` |

- 0.8.4 –: github.com/vitest-dev/vitest/releases/tag/v0.8.4: run tests depending on changed files (#1078)

#### vue-tsc

| Operation | 1.0.0 – |
|---|---|
| `typecheck` | `--noEmit` |
| `typecheck-build` | `--build` |

- 1.0.0 –: vuejs/language-tools changelogs/CHANGELOG-v1.md 1.0.0 (first stable); CHANGELOG.md 3.0.0: unchanged tsc command line

#### wrangler

| Operation | 3.0.0 – 4.59.0 | 4.59.0 – |
|---|---|---|
| `codegen-check` | cannot | `types {args} --check` |

- 3.0.0 – 4.59.0: packages/wrangler/CHANGELOG.md 4.59.0 (#11852): `--check` added to `wrangler types`
- 4.59.0 –: packages/wrangler/CHANGELOG.md 4.59.0 (#11852): Add `--check` flag to `wrangler types` command

#### yarn

| Operation | 2.0.0 – 2.4.0 | 2.4.0 – 3.0.0 | 3.0.0 – |
|---|---|---|---|
| `lock-check` | cannot | `install --immutable --skip-builds` | `install --immutable --mode=skip-build` |

- 2.0.0 – 2.4.0: yarnpkg/berry CHANGELOG.md 2.4.0 (--skip-builds is new: before, an immutable install runs build scripts)
- 2.4.0 – 3.0.0: yarnpkg/berry CHANGELOG.md 2.4.0 (new --skip-builds option on yarn install)
- 3.0.0 –: yarnpkg/berry CHANGELOG.md 3.0.0 (--skip-builds renamed into --mode=skip-build)
- A check command runs it itself (§16.1) when it matches ``(^\|[;&\|`!{]\|\$\(\|\b(?:then\|do\|else\|exec\|time)\s)\s*yarn(?=$\|[\s;&\|)`])``.

#### zizmor

| Operation | 1.30.1 – |
|---|---|
| `lint` | `--offline --no-progress --no-ignores --format github` |

- 1.30.1 –: zizmor 1.30.1: https://github.com/zizmorcore/zizmor/releases/tag/v1.30.1; https://docs.zizmor.sh/usage/#exit-codes
<!-- /generated:adapters -->

### Tests

- §16.1: `crates/forge-check/tests/spec_adapters.rs::a_check_running_an_adapted_tool_itself_is_rejected`, `crates/forge-packs/tests/loading.rs::target_kind_checks`.
- §16.2: `crates/forge-check/src/adapters.rs (unit)` (`every_adapter_loads`, `ranges_and_ops`, `args_by_rank`, `versions`, `version_facts_drop_the_least_recently_used`), `crates/forge-check/tests/spec_adapters.rs::ranges_are_consecutive_and_name_every_operation`, `::caller_arguments_by_rank`.
- §16.3: `crates/forge-check/tests/spec_adapters.rs` (`a_release_newer_than_proven_is_driven_as_the_newest_range`, `a_release_that_cannot_is_not_enforced_and_passes`, `a_usage_error_is_a_note_never_a_finding`, `an_absent_tool_is_skipped_with_a_note`), `crates/forge-cli/tests/check.rs::forge_do_drives_a_fake_tool_and_notes_what_it_cannot`. The `Tool versions` report lines: (no test of their text: `forge status` with stand-in tools is not covered here). The version facts cache: (no test).
- §16.4: `crates/forge-check/tests/spec_adapters.rs::spec_16_tables_are_generated_from_the_adapters`.
- Each adapter against real releases of its tool: (no test: the TypeScript Forge's opt-in release matrix was not ported; adapters are proven by the pack examples on the tools installed where the tests run, `crates/forge-cli/tests/check.rs::check_examples`). condor: `crates/forge-builtins-data/tests/dead_code.rs` runs `python/no-dead-code`'s examples on a stand-in (`tests/fake_condor.py`), and on the pinned cndr with `FORGE_REAL_CONDOR=1` (opt-in: it downloads through uvx); `::unused_imports_are_left_to_ruff_when_it_lints`; nesting and what blocks: `crates/forge-builtins-data/src/dead_code.rs` (unit).
