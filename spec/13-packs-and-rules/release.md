### release

Detected by `.changeset/config.json`, `release-please-config.json`, `.release-please-manifest.json`, `.releaserc`, `.releaserc.*`, `release.config.*`, `cliff.toml`, `towncrier.toml`, `changelog.d/*`, or a dependency on `semantic-release`, `@changesets/cli` (npm) or `towncrier` (PyPI).

| Rule | Targets |
|---|---|
| `release/no-hand-changelog` | `permission` ask: `Edit(**/CHANGELOG.md)` (the release tooling writes it) |
| `release/no-hand-version` | `check` lint, advisory (§10.7): `forge_advise forge_version_changed`: a changed `package.json` `version` or `pyproject.toml` `[project] version` (the split-off advisory half of the no-hand-changelog proposal) |
| `release/changeset-present` | `check` lint, advisory: `forge_advise forge_release_notes @python-tests`: with `.changeset/config.json`, a changed file of a package whose nearest `package.json` is not `"private": true` and no `.changeset/*.md` added; with `[tool.towncrier] directory` (`towncrier.toml` or `pyproject.toml`), a changed `.py` file outside `@python-tests` and that directory and no file added there |
