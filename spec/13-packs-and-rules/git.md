<!-- Generated from the built-in packs by crates/forge-packs/tests/spec_tables.rs; do not edit. Regenerate: FORGE_BLESS=1 cargo test -p forge-packs --test spec_tables -->

## 13. Packs and rules: `git`

### git

Repository hygiene, whatever the language.

Detected by: every repository.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `git/editorconfig` | When the repository has an .editorconfig, changed files that no formatter of this setup formats respect it. | any | check lint: `forge_editorconfig` |
| `git/generated-files` | A changed file marked generated (linguist-generated in .gitattributes, or @generated or "Code generated ... DO NOT EDIT." in its first 15 lines) is regenerated from its source, not edited by hand. | any | check lint: `forge_advise forge_generated_edits`<br>advice, never blocks |
| `git/large-files` | No file of the change over 1 MiB is added or grown unless Git LFS stores it (its filter attribute is lfs); a larger file is the user's decision: ask them. | any | check lint: `forge_large_files 1048576` |
| `git/no-build-artifacts` | No dependency, cache or build output enters the change (file role @build-artifacts: node_modules/, __pycache__/, *.pyc, .next/, .nuxt/, .svelte-kit/, .turbo/, .coverage, .DS_Store), nor a directory holding pyvenv.cfg (a virtualenv) or CACHEDIR.TAG (pytest, ruff, mypy and uv caches): add it to .gitignore. | any | check lint: `forge_artifacts @build-artifacts` |
| `git/no-conflict-markers` | No added line starts with a merge conflict marker (<<<<<<< , >>>>>>> , \|\|\|\|\|\|\| ), in any file. | any | grep forbid `^(<<<<<<<\|>>>>>>>\|\\|\\|\\|\\|\\|\\|\\|)( \|$)` |
