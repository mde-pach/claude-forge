---
title: Writing packs
description: The pack tree, the rule file, and how examples and spec tests become this documentation.
---

## The pack tree

Packs are directories, nested **language first, then package, then sub-package**. A pack's id is its path:

```
packs/
├── safety/                      # user level, not tied to a language
├── python/
│   ├── fastapi/
│   └── sqlalchemy/
│       └── alembic/             # python/sqlalchemy/alembic
└── typescript/
    ├── nextjs/
    │   └── react/               # React rules that only make sense in Next.js
    └── astro/
        └── react/               # React as Astro islands
```

A child pack is active only when its parent is, so a package can carry different rules under different parents. Each pack directory holds:

| Path | |
|---|---|
| `pack.yaml` | `id` (its path), `description`, `level`, optional `detect` and `requires` — see [Schemas](../../reference/schemas/#pack) |
| `rules/<name>.yaml` | one rule each; its id is `<pack id>/<name>` |
| `context.md` | optional, ≤ 10 lines of facts copied to `.claude/rules/` |
| `init/` | optional templates for `forge init` |
| `<child>/` | sub-packs |

## The rule file

```yaml
id: typescript/nextjs/react/client-hooks
description: Client-only React hooks in app/ need a 'use client' file.
scope: {language: typescript, files: ["app/**", "src/app/**"]}
enforce:
  - target: ast-grep
    mode: forbid
    rule: { … }
examples:
  match: [ … ]       # must trigger the rule
  no_match: [ … ]    # must not
```

Targets, in order of preference: `tool` (your linter or type checker, audited), `ast-grep`, `permission`, `sandbox`, `guard`, `check`. The [rule schema](../../reference/schemas/#rule) lists every field, and [6 · Rules and targets](../../spec/06-rules/) shows what each target becomes.

## Examples are tests, tests are docs

- Every rule's examples run in `forge test` and in the spec ([13 · Packs and rules](../../spec/13-packs/)), one test per rule. The pack pages show each rule's result from CI.
- Behaviour is specified by the tests in `tests/spec/`. A file starts with an `@spec <number> · <title>` comment, and its test titles are requirements. The site renders the file — titles, doc comments and test code — with the CI results. To document new behaviour, write the test.

## Contributing a pack

1. Add `packs/<language>/<package>/pack.yaml` and one file per rule, with `match` and `no_match` examples (including the near misses to keep allowed).
2. Run `bun run test`. Update generator snapshots with `FORGE_UPDATE_SNAPSHOTS=1` when the change is intended.
3. Preview with `bun run docs:dev`: the pack appears in the sidebar and its page follows your edits.
