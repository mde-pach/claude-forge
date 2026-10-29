### docs

Detected by `**/docs/**/*.{md,mdx,rst}`, `**/doc/**/*.{md,mdx,rst}`, `README*`, `packages/*/README*`. Not tied to a language: it reads Python, TypeScript and JavaScript sources.

| Rule | Targets |
|---|---|
| `docs/changed-identifiers` | `check` lint, advisory (§10.7): `forge_docs_mentions` on non-test Python, TypeScript and JavaScript files, when no doc changed. **Which names:** only those whose existing definition the change removed, renamed or changed (a definition line removed and not added back as it was: a signature, a type, a value); a parameter only when it is gone from its function. A name only added (a new function, its parameters) never counts: it cannot make docs stale. **Where:** only where the docs reference code: inline code spans, fenced code blocks, and qualified names in the text (`module.name`, `Class.attr`); a word of the prose never counts, however it is spelled. Private (`_x`), short (< 4 characters) and common names are left out. |
| `docs/adr-immutable` | `check` lint, advisory (§10.7): `forge_adr_edits` on `@adr` (adr-tools `.adr-dir`, else `docs/adr/`, `doc/adr/`, `docs/decisions/`): an ADR accepted in the base (MADR front matter `status: accepted`, or a `## Status` section starting with Accepted) whose change touches more than its status |
