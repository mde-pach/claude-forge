<!-- Generated from the built-in packs by crates/forge-packs/tests/spec_tables.rs; do not edit. Regenerate: FORGE_BLESS=1 cargo test -p forge-packs --test spec_tables -->

## 13. Packs and rules: `docs`

### docs

Documentation kept in step with the code it describes.

Detected by: a file matching `**/docs/**/*.{md,mdx,rst}`, `**/doc/**/*.{md,mdx,rst}`, `README*`, `packages/*/README*`.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `docs/adr-immutable` | An accepted architecture decision record (file role @adr, adr-tools' .adr-dir) is not rewritten; only its status may change (superseded, deprecated), and a new decision goes in a new ADR that supersedes it. Advisory, it blocks Stop once per turn. | any: `@adr` | check lint: `forge_advise forge_adr_edits`<br>advice, never blocks |
| `docs/changed-identifiers` | When a turn removes, renames or changes the existing definition of a public name (a function, class, exported declaration, option or parameter) that the docs reference as code (a code span, a fenced block, a qualified name like Settings.timeout), and changes no doc, the mentions are shown; a name only added, or a word of the prose, never counts. Advisory, it blocks Stop once per turn. | any: `**/*.{py,pyi,ts,tsx,mts,cts,js,jsx,mjs,cjs}`, `!@python-tests`, `!**/tests/**`, `!@ts-tests`, `!@e2e-tests` | check lint: `forge_advise forge_docs_mentions`<br>advice, never blocks |
