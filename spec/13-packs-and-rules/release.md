<!-- Generated from the built-in packs by crates/forge-packs/tests/spec_tables.rs; do not edit. Regenerate: FORGE_BLESS=1 cargo test -p forge-packs --test spec_tables -->

## 13. Packs and rules: `release`

### release

Releases whose changelog and versions are written by release tooling (Changesets, release-please, semantic-release, towncrier, git-cliff).

Detected by: a dependency on one of `semantic-release` (npm), `@changesets/cli` (npm), `towncrier` (PyPI); or a file matching `.changeset/config.json`, `release-please-config.json`, `.release-please-manifest.json`, `.releaserc`, `.releaserc.*`, `release.config.*`, `cliff.toml`, `towncrier.toml`, `changelog.d/*`.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `release/changeset-present` | With Changesets, a change to a package whose package.json is not "private": true adds a .changeset/*.md; with towncrier ([tool.towncrier] directory), a change to Python code outside the tests (file role @python-tests) adds a news fragment there. | any | check lint: `forge_advise forge_release_notes @python-tests`<br>advice, never blocks |
| `release/no-hand-changelog` | CHANGELOG.md is written by the release tooling: an Edit or Write of it needs confirmation. Add a changeset, a news fragment or a conventional commit instead. | any | permission ask `Edit(**/CHANGELOG.md)` |
| `release/no-hand-version` | A changed package.json version or pyproject.toml [project] version, which the release tooling sets; revert it unless the change is the release itself. | any: `**/package.json`, `**/pyproject.toml` | check lint: `forge_advise forge_version_changed` (when `**/package.json` or `**/pyproject.toml` changes)<br>advice, never blocks |
