## 13. Packs and rules

Rule ids are `<pack-id>/<name>`. Each line is one rule file. Headings are pack ids, so their nesting is the pack tree.


Packs by root:

- [safety](safety.md)
- [python](python.md)
- [typescript](typescript.md)
- [docs](docs.md)
- [github-actions](github-actions.md)
- [docker](docker.md)
- [secrets](secrets.md)
- [git](git.md)
- [kubernetes](kubernetes.md)
- [supply-chain](supply-chain.md)
- [gitlab-ci](gitlab-ci.md)
- [terraform](terraform.md)
- [ai-models](ai-models.md)
- [openapi](openapi.md)
- [sql](sql.md)
- [pre-commit](pre-commit.md)
- [release](release.md)
- [toolchain](toolchain.md)

### 13.1 Migration linting

`migration-lint` checks run [Squawk](https://squawkhq.com) (`squawk-cli`, pinned, through `bunx` or `uvx`, like ast-grep) on migration SQL and fail only on the changes that break the running version of the application during a deploy: `renaming-column`, `renaming-table`, `changing-column-type`, `adding-required-field`, `adding-not-nullable-field`, `ban-drop-database`. Squawk's other advice (lock and statement timeouts, `IF NOT EXISTS`, `CONCURRENTLY`, column types) is not enforced: Prisma and Alembic generate SQL that triggers it on every migration. Findings are printed `file:line:column` (from 1). The project cannot silence Squawk: it runs with Forge's own config (an empty `.squawk.toml` passed with `-c`, so the project's `excluded_rules` and `excluded_paths` do not apply) on copies of the SQL whose `squawk-ignore` / `squawk-ignore-file` comments are rewritten into plain comments (same lines and columns); a rule is disabled in `forge.yaml`. When Squawk cannot run (offline, no `bunx` or `uvx`) or Alembic cannot render a revision, the check prints `forge:<id> skipped: …` and does not block. In CI (`--all`) only the newest migration is linted, so an accepted rename does not fail every later run.


### 13.2 zizmor

`forge_zizmor <audit>…` runs [zizmor](https://docs.zizmor.sh) pinned (`uvx zizmor@<pin>`), `--offline` (no GitHub token, no network audits), `--no-ignores` (inline `# zizmor: ignore` comments and ignore rules in its config do not apply; a rule is disabled in `forge.yaml`) and `--format github`, on the changed in-scope files (with `--all`, or when only a `when_changed` file changed, every tracked file of the rule's scope: workflows, actions, `.github/dependabot.y*ml`, a pre-commit config when a rule lists it). Findings of the named audits print as `<path>:<line> <audit>: <message>` and are line-attributed like any lint check. When uvx is missing or zizmor cannot run (offline without a cached copy), the check prints `forge:<id> skipped: …` and does not block.


### 13.3 hadolint

`forge_hadolint <code>…` runs hadolint (pinned, from the `hadolint-bin` wheel through `uvx --from hadolint-bin==<pin>`) once per `check.sh` run on the Dockerfiles of the change, shared by the hadolint rules. Before running it, it hashes the binary and compares it with the upstream release's sha256 for the platform (embedded in Forge); a mismatch, an unknown platform, or no uvx prints `forge:<id> skipped: …` and does not block. It runs with `-c <file containing "ignored: []">` (the project's `.hadolint.yaml` ignores do not apply), `--disable-ignore-pragma` (inline `# hadolint ignore=` comments do not apply; a rule is disabled in `forge.yaml`), `--no-fail --no-color -f tty`; lines `path:line CODE level: message` of the named codes print as `path:line CODE: message`. `forge test` also checks the wheel's binary against the upstream checksum.


### 13.4 Libraries and project-run helpers

`detect.library_any` activates a pack when the project publishes a library to that ecosystem. The shape of a package is not enough (most uv workspace members and packaged services have a build backend; internal workspace packages declare `exports`), so detection also needs **evidence that it is published**; without it, add the pack in `forge.yaml`. Each `pyproject.toml` / `package.json` (the root's and each workspace package's) is judged on its own. **pypi:** `pyproject.toml` has `[project] name` and a `[build-system]` table (`uv init --lib`; `uv init` makes an application without one), no `[project.scripts]` or `[project.gui-scripts]` (`uv init --package`: a packaged application), no `Private ::` classifier, no `manage.py` in its package (for the root: outside the workspace packages), and is published: it has trove `classifiers` (PyPI metadata), or a CI workflow (`.github/workflows/*.yml`, `.gitlab-ci.yml`) uploads to PyPI (`pypa/gh-action-pypi-publish`, `uv`/`poetry`/`hatch`/`flit`/`pdm publish`, `twine upload`). **npm:** `package.json` is not `"private": true`, has a `name`, has `exports`, or `main` / `module` together with `types`, `typings` or `files` (`main` alone is `npm init`'s default), and is published: `publishConfig`, Changesets (`.changeset/config.json`), or a CI workflow running `npm`/`pnpm`/`yarn`/`bun publish`, `changesets/action` or `semantic-release`. A library that ships a CLI script is not detected; add the pack in `forge.yaml`.

`forge_helper <runtime> <script> [args]` runs `.forge/helpers/<script>` (written when an active check uses it, like the ratchet's) with the project's `python` or `bun`, as `<script> <FORGE_TMP> [args]`, where the change set (`changed`, `inbase`, `base/`) is; findings are `path:line message` (line 0: the file as a whole), attributed like lint diagnostics; a finding line found in `forge.yaml` is accepted; exit 3 prints `skipped:`.

Forge has one release cooldown, `FORGE_COOLDOWN_SECONDS` in the generated scripts (259200: 3 days): the minimum age the package managers' cooldown settings must ask for (`forge_bun_cooldown`, `forge_uv_cooldown`, `pm_policy.mjs npm-cooldown`) and the minimum age of a newly added package (`new_deps.mjs`, `new_deps.py`). The registry helpers read `FORGE_REGISTRY_NPM` / `FORGE_REGISTRY_PYPI` as a mirror of registry.npmjs.org / pypi.org when set.

- `drf_permissions.py`: §13 `python/django/drf/permission-classes`.
- `fk_index.py`: §13 `python/sqlalchemy/fk-index`: in each changed `.py` file outside `versions/`, the foreign key columns (a `ForeignKey(…)` argument of `mapped_column` / `Column`, a `ForeignKeyConstraint` in `__table_args__`; per class or `Table(…)`) not in the base copy that no index leads with: `index=True` / `unique=True`, the first `primary_key=True` column, or the first column of an `Index` (after its name), `UniqueConstraint` or `PrimaryKeyConstraint`. Reported at the column's line.
- `pytest_filterwarnings.py <entry> <dependency>`: §13 `python/pytest/*-deprecations-error`: the pytest configuration pytest uses (the first of `pytest.ini`, `.pytest.ini`, `pyproject.toml` with `[tool.pytest]` or `[tool.pytest.ini_options]`, `tox.ini` with `[pytest]`, `setup.cfg` with `[tool:pytest]`) has `<entry>` or a bare `error` in `filterwarnings` (spaces ignored); otherwise a finding at line 0 of that file.
- `public_api.py`: for each changed module whose base copy has a literal `__all__`, each name that left it, or every name of a deleted module, must still be defined with a deprecation, read from the syntax tree: a `def` of that name decorated with `@deprecated` (`typing_extensions`, `warnings`) or calling `warnings.warn(…, DeprecationWarning | FutureWarning | PendingDeprecationWarning)` (or a subclass defined in the project, or a category parameter defaulting to one); a `class` with that decorator or such a call in `__init__` / `__new__` / `__init_subclass__` / `__post_init__`; an alias `name = deprecated(…)(…)`; or a module `__getattr__` that warns that way and names it (or reads a module-level table naming it). A project function that warns that way counts when the shim calls it or decorates with it. The word in a docstring or a comment does not count. A major version bump does not allow a removal by itself (the agent could make it): an intended removal is accepted in `forge.yaml`, which asks.
- `public_api.mjs removed`: the names the package's entry points (`exports`, `main`, `module`, `types`, with `dist/`, `lib/`, `build/`, … mapped to `src/` and `.js` / `.d.ts` to the source file; `export *` followed through relative modules) export in the base are still exported, a major version bump included (an intended removal is accepted in `forge.yaml`); reported at line 0 of the module that declared the name. A name that became an alias of another exported name (`export const old = renamed`, `export { renamed as old }`; it was not one in the base) needs a `/** … @deprecated … */` JSDoc block right before its export statement or specifier.


### 13.5 Pinned release binaries and system tools

Tools with no PyPI or npm distribution run from their GitHub release binaries, pinned like Forge's other tools (the newest release older than 14 days) by version and by an embedded sha256 per platform (`uname -s`-`uname -m`: Linux and macOS on x86_64 and arm64), computed from the release assets and matching both GitHub's asset digest and the release's checksum file: actionlint 1.7.12, gitleaks 8.30.1, kubeconform 0.8.0, kube-linter 0.8.3, tflint 0.64.0, trivy 0.75.0, osv-scanner 2.7.0, oasdiff 1.33.0, editorconfig-checker 4.0.2 (`RELEASE_BINARIES` in `src/util.ts`). `forge_release_bin <name>` (and `forge_release <name> <var>`, which prints the check's skipped line) is the one way a helper gets such a binary: the first use on a machine downloads the asset with `curl` (or `wget`) from `check.sh`, which hooks, pre-commit and CI run outside the Bash sandbox; it checks the asset's sha256, unpacks the binary (`.tar.gz`, `.zip` with `unzip`, or the asset itself) and keeps it in `${XDG_CACHE_HOME:-~/.cache}/forge/bin/<name>-<version>-<platform>/<name>` (written to a temporary name and renamed, so concurrent runs never see half a file). No asset for the platform, no network on the first use, no `curl`/`wget` (or `unzip`), or a sha256 mismatch prints `forge:<id> skipped: …`, never a pass. A helper's skipped line under `forge_once` / `forge_advise` is printed, not counted as a finding.

System tools a project installs outside `node_modules` and `.venv` (`terraform`, `tofu`, `helm`, `kustomize`, `kubectl`) and the package managers (`npm`, `pnpm`, `yarn`) are found on `PATH`, like the runtimes (`uv`, `bun`, `docker`), never as a shell function of that name. `yarn` counts only as Yarn 2 or later (`yarn --version` in the repository: a `.yarnrc.yml` project's `yarnPath` answers there), the Yarn the `typescript/yarn` pack is for; Yarn 1 is reported as `yarn 2 or later not installed`. A check that names `terraform` runs the project's Terraform CLI: `tofu` when the repository has a `*.tofu` file or a `.terraform.lock.hcl` naming `registry.opentofu.org` providers, else `terraform`; `kustomize` is `kubectl kustomize` when only kubectl is installed. A missing one is reported like any missing tool, by the binary's name (`forge:<id> skipped: tofu not installed`, `forge status`, SessionStart).

**ruff.** Check commands name `ruff`; the generated scripts run it as `forge_ruff`, the project's own ruff (§9.1) as the commands give it, once. Before 0.2.0 ruff has no `concise` output format, and its `text` format is the same one line per diagnostic: a command naming `concise` that ruff rejects runs again with `text`. A command the project's ruff rejects as usage (exit 2 with its argument parser's `For more information, try '--help'.`) is not a finding: the check prints `forge:<id> skipped: ruff <version> does not accept this command (ruff <args>): <ruff's error>` and passes (formatting a base copy on stdin fails instead, so the plain comparison of §10.1 applies).
