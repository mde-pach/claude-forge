# Forge

**Guardrails for Claude Code, generated from rule packs.**

Forge reads rules grouped in packs and writes a standalone Claude Code setup — permissions, sandbox, hooks and a `check.sh` — for your user account or for one git repository. Once written, it runs with plain Claude Code: no generated file calls Forge.

📖 **Documentation: https://mde-pach.github.io/claude-forge/** — a page per pack, generated from [`packs/`](packs/), showing what each rule does at each step of the [lifecycle](https://mde-pach.github.io/claude-forge/lifecycle/). Every behaviour on the site is a test from [`tests/spec/`](tests/spec/), shown where it applies with its latest CI result.

```sh
bunx @maximedepachtere/forge apply --user   # once per machine
bunx @maximedepachtere/forge apply          # in each repository
```

- **Standalone output.** POSIX `sh`, `git`, `grep`, `sed`, `awk` and your project's own tools.
- **Enforced in the closest native place:** your linter and type checker, then ast-grep, then Claude Code permissions and the sandbox, then hooks.
- **Claude cannot change the setup unnoticed.** Forge-owned files are behind a permission prompt and hash-checked at every Stop, pre-commit and CI run.
- **Rules are data.** One YAML file per rule, in a tree of packs (language → package → sub-package, e.g. `typescript/nextjs/react`), with examples that double as tests.

Stack: uv, FastAPI, SQLAlchemy 2 + Alembic, Pydantic v2, ruff, basedpyright, pytest — Bun, Next.js 16 + React 19, Astro (+ React islands), Prisma 7, Zod, Biome 2, tsc, Vitest, Playwright. Projects using other tools are never broken: rules whose tool is absent are reported as *not enforced here*.

## Development

```sh
bun install
bun run test          # the executable specification (tests/spec/)
bun run test:report   # same, writing JUnit results to test-results/ for the docs
bun run docs:dev      # Astro + Starlight + React site, live on packs/ and tests/spec/
```

The specification is [`SPECS.md`](SPECS.md); the build state and deliberate departures are in [`HANDOFF.md`](HANDOFF.md).
