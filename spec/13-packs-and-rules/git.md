<!-- Generated from the built-in packs by crates/forge-packs/tests/spec_tables.rs; do not edit. Regenerate: FORGE_BLESS=1 cargo test -p forge-packs --test spec_tables -->

## 13. Packs and rules: `git`

### git

Repository hygiene, whatever the language.

Detected by: every repository.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `git/editorconfig` | When the repository has an .editorconfig, changed files that no formatter of this setup formats respect it. | any | check lint: `forge_editorconfig` |
| `git/generated-files` | A changed file marked generated (linguist-generated in .gitattributes, or @generated or "Code generated ... DO NOT EDIT." in its first 15 lines) is regenerated from its source, not edited by hand. | any | check lint: `forge_advise forge_generated_edits`<br>advice, never blocks |
| `git/large-files` | No file over 1 MiB is added, nor taken past 1 MiB, unless Git LFS stores it (its filter attribute is lfs); lock files are exempt; a larger file is the user's decision: ask them. | any | check lint: `forge_large_files 1048576` |
| `git/no-build-artifacts` | No dependency, cache or build output enters the change (file role @build-artifacts: node_modules/, __pycache__/, *.pyc, .next/, .nuxt/, .svelte-kit/, .turbo/, .coverage, .DS_Store), nor a directory holding pyvenv.cfg (a virtualenv) or CACHEDIR.TAG (pytest, ruff, mypy and uv caches): add it to .gitignore. | any | check lint: `forge_artifacts @build-artifacts` |
| `git/no-conflict-markers` | No added line starts with a merge conflict marker (<<<<<<< , >>>>>>> , \|\|\|\|\|\|\| ), in any file. | any | grep forbid `^(<<<<<<<\|>>>>>>>\|\\|\\|\\|\\|\\|\\|\\|)( \|$)` |
| `git/no-token-in-remote-url` | A git remote URL carries no credentials (https://user:token@host, https://<token>@host, ?token=): git stores the URL in .git/config in clear text and prints it in its output. Authenticate through a credential helper (gh auth setup-git, git credential-manager) or SSH. | any | guard deny: command `{name: git, sub: [clone\|fetch\|pull\|push\|ls-remote\|submodule], arg_regex: '^https?://([^/@:[:space:]]*:[^/@[:space:]]+\|(gh[pousr]_\|github_pat_\|glpat-\|\$)[^/@:[:space:]]*)@\|[?&](access_\|private_)?token='}` or `{name: git, sub: [remote, add\|set-url], arg_regex: '^https?://([^/@:[:space:]]*:[^/@[:space:]]+\|(gh[pousr]_\|github_pat_\|glpat-\|\$)[^/@:[:space:]]*)@\|[?&](access_\|private_)?token='}` |

### git/dco

A repository whose contributions need a Developer Certificate of Origin sign-off.

Detected by: a file matching `.github/dco.yml`; or a line matching `Developer Certificate of Origin\|Signed-off-by:` in a file matching `CONTRIBUTING*`, `.github/CONTRIBUTING*`, `docs/CONTRIBUTING*`.
Child of `git`: active only where it is.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `git/dco/signoff` | Commits in this repository carry a Developer Certificate of Origin sign-off: commit with git commit -s (--signoff), which adds the Signed-off-by trailer of the configured git user. | any | guard deny: command `{name: git, sub: [commit], flags_none: [-s, --signoff, --amend]}` |
