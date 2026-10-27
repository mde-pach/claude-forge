### docs

Detected by `**/docs/**/*.{md,mdx,rst}`, `**/doc/**/*.{md,mdx,rst}`, `README*`, `packages/*/README*`. Not tied to a language: it reads Python, TypeScript and JavaScript sources.

| Rule | Targets |
|---|---|
| `docs/changed-identifiers` | `check` lint, advisory (§10.7): `forge_docs_mentions` on non-test Python, TypeScript and JavaScript files |
| `docs/adr-immutable` | `check` lint, advisory (§10.7): `forge_adr_edits` on `@adr` (adr-tools `.adr-dir`, else `docs/adr/`, `doc/adr/`, `docs/decisions/`): an ADR accepted in the base (MADR front matter `status: accepted`, or a `## Status` section starting with Accepted) whose change touches more than its status |
