<!-- Generated from the built-in packs by crates/forge-packs/tests/spec_tables.rs; do not edit. Regenerate: FORGE_BLESS=1 cargo test -p forge-packs --test spec_tables -->

## 13. Packs and rules: `pre-commit`

### pre-commit

pre-commit hook configurations (.pre-commit-config.yaml).

Detected by: a file matching `.pre-commit-config.yaml`.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `pre-commit/pinned-revs` | Every repos[].rev in .pre-commit-config.yaml is a tag or a commit SHA, never a branch (main, master, HEAD, trunk, develop, latest): a branch runs whatever the hook repository pushes next, on every developer machine. pre-commit autoupdate --freeze pins SHAs. | any: `.pre-commit-config.yaml` | grep forbid `^[[:space:]]*-?[[:space:]]*rev:[[:space:]]*["']?(main\|master\|HEAD\|trunk\|develop\|latest)["']?[[:space:]]*(#.*)?$` |
