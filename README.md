# Forge

**Guardrails for Claude Code, generated from rule packs.**

Forge reads rules grouped in packs and writes a standalone Claude Code setup — permissions, sandbox, hooks and a `check.sh` — for your user account or for one git repository. Once written, it runs with plain Claude Code: no generated file calls Forge.

📖 **Documentation: https://mde-pach.github.io/claude-forge/** — a page per pack, generated from [`packs/`](packs/), showing what each rule does at each step of the [lifecycle](https://mde-pach.github.io/claude-forge/lifecycle/). Every behaviour on the site is a test from [`tests/spec/`](tests/spec/), shown where it applies with its latest CI result.

Forge is not on npm yet, so run it from a clone:

```sh
git clone https://github.com/mde-pach/claude-forge.git ~/src/claude-forge
cd ~/src/claude-forge && bun install && bun run build
alias forge='node ~/src/claude-forge/dist/forge.js'   # or: bun link, which puts forge on PATH

forge apply --user      # once per machine
cd my-app && forge apply  # in each repository; --yes answers its questions
```

Once published, `bunx @maximedepachtere/forge …` or `npx @maximedepachtere/forge …` replace `forge`. [Getting started](https://mde-pach.github.io/claude-forge/guide/getting-started/) covers the pre-commit hook, `--local`, and repositories where no pack is detected.

- **Standalone output.** POSIX `sh`, `git`, `grep`, `sed`, `awk` and your project's own tools.
- **Enforced in the closest native place:** your linter and type checker, then ast-grep, then Claude Code permissions and the sandbox, then hooks.
- **Claude cannot change the setup unnoticed.** Forge-owned files are behind a permission prompt and hash-checked at every Stop, pre-commit and CI run.
- **Rules are data.** One YAML file per rule, in a tree of packs (language → package → sub-package, e.g. `typescript/nextjs/react`), with examples that double as tests.

Stack: uv, FastAPI, SQLAlchemy 2 + Alembic, Pydantic v2, ruff, ty, pytest — Bun, Next.js 16 + React 19, Astro (+ React islands), Prisma 7, Zod, Biome 2, tsc, Vitest, Playwright. Projects using other tools are never broken: rules whose tool is absent are reported as *not enforced here*.

## Development

```sh
bun install
bun run test          # the executable specification (tests/spec/)
bun run test:report   # same, writing JUnit results to test-results/ for the docs
bun run docs:dev      # Astro + Starlight + React site, live on packs/ and tests/spec/
```

The specification is [`SPECS.md`](SPECS.md), an index of [`spec/`](spec/) (one file per section, numbered like `tests/spec/`); the build state and deliberate departures are in [`HANDOFF.md`](HANDOFF.md); how to add a rule, pack, target kind or engine helper is in [`CONTRIBUTING.md`](CONTRIBUTING.md).

```sh
bun run typecheck     # src, tests
bun run lint          # Biome (format and lint) and shellcheck on the script templates
bun run format        # Biome's formatting, written
```
