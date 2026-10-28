# Contributing to Forge

Forge reads rules and writes a Claude Code setup. This page says where each kind of change goes. The specification is [`SPECS.md`](SPECS.md) (an index of [`spec/`](spec/)); its tests are `tests/spec/`, one requirement per test title. A change to behaviour changes both.

## Before you push

```sh
bun install
bun run typecheck          # src and tests
bun run lint               # Biome on the TypeScript, shellcheck on templates/
bun run build && bun run test   # the specification; a single file: bun tests/run.ts tests/spec/10-check.test.ts
```

Generated output is pinned by `tests/generator/__snapshots__/`. After a change that alters what `forge apply` writes, regenerate them with `FORGE_UPDATE_SNAPSHOTS=1 bun tests/run.ts tests/spec/14-2-generator.test.ts` and read the diff: it should show only what you meant to change. Engine modules and verbatim templates appear as one line each (`@@ templates/engine/sql.sh @@`).

Formatting is Biome's (`bun run format`). Commits that only reformat are listed in `.git-blame-ignore-revs`.

## Where things live

| You want to change | Where |
|---|---|
| A rule, or a new one | `packs/<pack>/rules/<name>.yaml` (YAML only) |
| A pack, or a new one | `packs/<path>/pack.yaml`, `context.md`, `rules/`, `init/` |
| What a kind of target (`tool`, `ast-grep`, `guard`, …) does | `src/targets/<kind>.ts` |
| A shell helper a check calls (`forge_hadolint`, `forge_squawk_files`, …) | `templates/engine/<module>.sh` |
| How a release of a tool is invoked, which codes it has | `src/adapters/<tool>.ts` (§16) |
| A built-in guard check (`check: ephemeral-exec`) | `templates/scripts/guard.sh` and `GUARD_CHECKS` in `src/core/vocabulary.ts` |
| A file role (`@python-tests`) | `ROLES` in `src/roles.ts` |
| Which packs are active, where rules apply, what gets written | `src/plan/` |
| How generated text is produced | `src/render/` |
| A command or its output | `src/cli/commands/<command>.ts`, shared pieces in `src/cli/` |
| Files, globs, versions, paths, tool lists | `src/core/` |

## Recipes

### A new rule

Add `packs/<pack>/rules/<name>.yaml` with `id: <pack>/<name>`, a one-sentence `description`, `scope`, `enforce` and `examples` (`match` and `no_match`). Prefer, in order: a `tool` target (the project's linter already has the diagnostic) with an `ast-grep` stand-in, an `ast-grep` target, a `grep` target for text ast-grep cannot parse, a `check`, a `permission`, a `guard`. The examples are its tests: `bun run test:packs` runs them. Add the rule's line to its pack's page in `spec/13-packs-and-rules/`. The [writing packs](https://mde-pach.github.io/claude-forge/contributing/writing-packs/) page has the details.

### A new pack

Add a directory with a `pack.yaml` (`id` equal to its path, `level`, `detect`, optionally `requires` and `tool_off`). A child pack is a sub-directory with its own `pack.yaml`; it is active only under an active parent. `context.md` is at most 10 lines of facts. Add a sample under `tests/generator/` when the pack changes what a typical project gets.

### A new target kind

Add `src/targets/<kind>.ts` implementing `TargetKind` (`src/targets/kind.ts`):

- `schema`: the JSON Schema of its fields;
- `phase`: when the plan handles it (2 unless it interacts with `tool` or `ast-grep`);
- `plan`: what it adds to the plan (`ctx.out`) and says in `forge status` (`st.enforced`, `st.notEnforced`);
- `enforcement`: what `forge status` lists as enforcing the rule;
- `lifecycle`: its events on the docs' lifecycle pages;
- `selftest`: how its examples are run;
- `validate` and `needsExamples` as needed.

Then add it to `TARGETS` in `src/targets/index.ts` and its type to `Target` in `src/types.ts`. The rule schema, the plan, `forge status`, the self-test and the docs pick it up. If it renders a file of its own, add the renderer to `src/render/` and write the file in `src/plan/files.ts`. Specify it in `spec/06-rules.md` and test it in `tests/spec/06-rules.test.ts`.

### A new engine helper

Add the function to the module of its concern in `templates/engine/` (or a new `templates/engine/<module>.sh` starting with `# @module <name>` and a line on what it is for). Name it `forge_<name>` and its constants `FORGE_<NAME>`: that is how Forge finds which module provides it. A check's `run` calls it by name, and a project then gets that module and the ones it calls. Nothing is listed by hand. Version pins go in as `@@PLACEHOLDER@@`, filled by `pins()` in `src/render/engine.ts`. `core.sh` documents the state a helper may read (`$FORGE_TMP/changed`, `existing`, `base/`, `added`, `$c_id`). The pack test "every check target, rendered on its own, has every engine function it calls" fails if a helper's module cannot be found.

### A tool adapter, or a new release of a tool

A check asks a tool for an operation, `forge_do <tool> <operation> {files}`, never with a command line of its own (§16). The tool's adapter, `src/adapters/<tool>.ts`, lists version ranges, each citing the changelog entry that makes it a range, with the arguments of each operation there (`null`: that range cannot). Add it to `ADAPTERS` in `src/adapters/index.ts`, and its releases to the test matrix (`TOOL_VERSIONS` in `tests/helpers.ts`): `tests/spec/16-adapters.test.ts` runs every operation on each. When a release changes something, add a range. When the tool has a new release (the test comparing with PyPI fails), add it to the matrix (ruff: `RUFF_MATRIX`) and run `bun scripts/ruff-rules.ts`, which reads each release's own rule table and finds from which one each code reports its rule's examples.

### A new built-in guard check

Add `guard_check_<name>()` to `templates/scripts/guard.sh` and `<name>` to `GUARD_CHECKS` in `src/core/vocabulary.ts`. The schema then accepts `check: <name>` on a guard target, and `99-internals` checks that the function exists.

### A new file role

Add it to `ROLES` in `src/roles.ts`: what it is, its default globs, and, when the tool's config is data Forge can read, a function reading it. Rules then name it as `@<role>`. List it in `spec/15-engine-capabilities.md` (§15.4).

## Structure rules the tests keep

- `src/` has no import cycles (`99-internals`).
- The JSON Schemas and the TypeScript types name the same fields (`src/schemas.ts`, at compile time).
- The vocabulary (levels, stages, modes, decisions, …) is defined once, in `src/core/vocabulary.ts`.
- Generated files never call Forge; the scripts are POSIX `sh` (`shellcheck`, and the guard on sh, dash, bash and BusyBox).
