<!-- Generated from the built-in packs by crates/forge-packs/tests/spec_tables.rs; do not edit. Regenerate: FORGE_BLESS=1 cargo test -p forge-packs --test spec_tables -->

## 13. Packs and rules: `code`

### code

Norms for changing code, shared by the language packs.

Detected by: every repository.

Context (in `.claude/rules/forge-*.md`, §7):

- `reuse-helpers` (when pack `python` or `typescript` active) Before writing a utility, search the codebase for an existing helper (warn-once and logging helpers, `utils` functions such as an existing property setter) and reuse it instead of adding a near-duplicate.
- `test-integrity` (when pack `python/pytest`, `typescript/vitest`, `typescript/playwright`, `typescript/bun` or `typescript/jest` active) Tests assert behaviour: never weaken, remove or skip an assertion to make a test pass.
- `test-edits` (when pack `python/pytest`, `typescript/vitest`, `typescript/playwright`, `typescript/bun` or `typescript/jest` active) When the task changes behaviour an existing test asserts, update that test in place with Edit or Write (keep its name) and say so in your summary. Otherwise never edit an existing test to make the code pass.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `code/no-versioned-duplicates` | Code is changed in place, not copied: no new foo-v2.ts, foo_new.py, foo-old.tsx, Button copy.tsx or foo.ts.bak beside the foo it copies, which leaves two versions to maintain and the old one still imported. Edit the original and delete the copy, unless the task asks for both side by side (an API v2). At Stop it blocks once per turn, then accepts. | any: `**/*.{ts,tsx,mts,cts,js,jsx,mjs,cjs,py,vue,svelte,astro}`, `**/*.bak`, `**/*.orig`, `!@build-artifacts`, `!**/migrations/**` | check lint: a shell script calling `forge_advise`, `forge_new_files`<br>advice, never blocks |

### code/jscpd

The project's own jscpd (duplicated blocks), as its config sets it up.

Detected by: a file matching `.jscpd.json`; or a line matching `^\s*"jscpd"\s*:\s*\{` in a file matching `package.json`.
Child of `code`: active only where it is.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `code/jscpd/duplicates` | The change does not copy code the project's jscpd finds duplicated (with its own config): put the shared code in one function or module and use it from both places. A duplicate is advice, and fails when the project's jscpd config fails on it (the duplication over its `threshold`, or an `exitCode`). | any | check lint: `forge_jscpd` |
