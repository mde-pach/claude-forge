<!-- Generated from the built-in packs by crates/forge-packs/tests/spec_tables.rs; do not edit. Regenerate: FORGE_BLESS=1 cargo test -p forge-packs --test spec_tables -->

## 13. Packs and rules: `code`

### code

Norms for changing code, shared by the language packs.

Detected by: every repository.

Context (in `.claude/rules/forge-*.md`, §7):

- `reuse-helpers` (when pack `python` or `typescript` active) Before writing a utility, search the codebase for an existing helper (warn-once and logging helpers, `utils` functions such as an existing property setter) and reuse it instead of adding a near-duplicate.
- `test-integrity` (when pack `python/pytest`, `typescript/vitest`, `typescript/playwright`, `typescript/bun` or `typescript/jest` active) Tests assert behaviour: never weaken, remove or skip an assertion to make a test pass.
- `test-edits` (when pack `python/pytest`, `typescript/vitest`, `typescript/playwright`, `typescript/bun` or `typescript/jest` active) When the task changes behaviour an existing test asserts, update that test in place with Edit or Write (keep its name) and say so in your summary. Otherwise never edit an existing test to make the code pass.

No rules of its own.
