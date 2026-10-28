### git

Active in every repository (`detect: {always: true}`, §5.1).

| Rule | Targets |
|---|---|
| `git/no-conflict-markers` | `grep` forbid, any file: `^(<<<<<<<\|>>>>>>>\|\|\|\|\|\|\|\|)( \|$)` (`=======` is left out: a Markdown or reStructuredText heading underline) |
| `git/large-files` | `check` lint: `forge_large_files 1048576`: a file of the change over 1 MiB, new or grown, whose `git check-attr filter` is not `lfs` (line 0); the threshold is the argument (replace the rule locally). Proved by a spec test: its files do not fit in examples |
| `git/generated-files` | `check` lint, advisory (§10.7): `forge_generated_edits`: a changed file marked `linguist-generated` (`git check-attr`) or with `@generated` / `Code generated … DO NOT EDIT.` in its first 15 lines: regenerate it rather than edit it |
| `git/editorconfig` | `check` lint: `forge_editorconfig`, when `.editorconfig` exists: pinned editorconfig-checker (§13.5) with an empty config of Forge's (`.editorconfig-checker.json` excludes and disables do not apply), `-format gcc`, on the changed files no format check of the setup covers |
| `git/no-build-artifacts` | `check` lint: `forge_artifacts @build-artifacts`: each file of the change (ignored files are not part of it) in the role, or under a directory holding `pyvenv.cfg` (a virtualenv) or `CACHEDIR.TAG` (a tagged cache: pytest, ruff, mypy, uv), reported once per shallowest such directory: add it to `.gitignore` |
