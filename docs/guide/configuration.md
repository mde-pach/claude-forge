# Configuration

Most projects need no configuration: Forge detects the packs from your manifests and lock files. Two optional inputs let you adjust that. Both are **inputs, not output**: `forge apply` never rewrites them, and Claude must ask before editing them (`Edit(forge.yaml)` and `Edit(.forge/**)` are in `permissions.ask`).

## `forge.yaml`

```yaml
# <repo>/forge.yaml
forge: "0.5"
packs:
  add: [zod]               # activate a pack detection missed
  remove: [playwright]     # deactivate a detected pack
disable:
  - id: pytest/run
    reason: The suite needs a GPU; it runs in CI only.
```

- `packs.add` activates a pack and the packs it requires.
- `packs.remove` deactivates a pack, and every pack that requires it (`forge status` says why).
- `disable` turns one rule off. The `reason` is required and shown by `forge status`: every rule blocks, so a rule you don't want is disabled explicitly, never silently ignored.

Unknown packs, user-level packs in `packs.add`, and disables of rules that are not active are reported as warnings. A file that does not match the schema stops `apply`.

## Project-local rules

Put rule files in `.forge/rules/<name>.yaml`. They use the [rule format](/guide/authoring) and are tested by `forge test` like pack rules.

- A local rule whose `id` equals a pack rule's `id` **replaces it entirely**.
- A local rule with a new id (for example `local/no-print`) is added.

```yaml
# .forge/rules/no-ts-ignore.yaml — stricter than the pack rule
id: typescript/no-ts-ignore
description: No @ts-ignore or @ts-nocheck.
scope: {language: typescript}
enforce:
  - target: ast-grep
    mode: forbid
    rule: {kind: comment, regex: "^//[[:space:]]*@ts-(ignore|nocheck)"}
examples:
  match: ["// @ts-nocheck\nconst a = 1;\n"]
  no_match: ["// ok\nconst a = 1;\n"]
```

## Detection

Detection reads `pyproject.toml` (project, optional and dependency-group dependencies), `uv.lock`, `package.json` and `bun.lock`, plus the files in the repository. Each pack's page lists what activates it — see [Packs](/packs/).

## Tool presence

Whether a tool is used by the project decides which targets apply:

| Tool | Present when |
|---|---|
| Biome | `biome.json`, `biome.jsonc`, or an `@biomejs/biome` dependency |
| ruff | `ruff.toml`, `.ruff.toml`, `[tool.ruff]` in `pyproject.toml`, or a `ruff` dependency |
| basedpyright | a `basedpyright` dependency, `[tool.basedpyright]`, or `pyrightconfig.json` |
| tsc | `tsconfig.json` or a `typescript` dependency |

A `check` that runs an absent tool is not generated and is reported as *not enforced here*. A tool that is configured but not installed yet (no `node_modules/.bin/…` or `.venv/bin/…`) is generated and skipped at run time with `forge:<id> skipped: <tool> not installed`.

## Databases

Migration checks marked `requires_local_db` run only when the configured database is on your machine. Forge looks for `DATABASE_URL` in the environment, then in `.env`, then `sqlalchemy.url` in `alembic.ini`, and treats `localhost`, `127.0.0.1`, `::1`, an empty host or a Unix socket as local. Anything else prints `forge:<id> skipped: no local database` and never blocks.
