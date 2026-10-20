# Forge — hand-off notes

Build state of Forge against `SPECS.md` (v0.5), as of 2026-10-20. Read the spec first, then this file.

## Toolchain

- Bun 1.3, Node ≥ 20, uv, Claude Code ≥ 2.1.295.
- Dev dependencies (`package.json`): `yaml`, `ajv` (2020-12), `jsonc-parser`, `semver`, `smol-toml`, `typescript`. Run `bun install`.
- Build: `bun run build` → `dist/forge.js`. The `scripts/postbuild.mjs` it references does not exist yet; it should add the `#!/usr/bin/env node` shebang and chmod.
- ast-grep pinned to **0.45.3**.

## Done

| Area | Files |
|---|---|
| Types | `src/types.ts` |
| Strict YAML (rejects duplicate keys, anchors, aliases, explicit tags; tested) | `src/yaml.ts` |
| JSON Schemas for pack, rule, forge.yaml + `validate()` | `src/schemas.ts` (not yet exercised; Ajv strict mode may need tweaks) |
| Pack, rule, local-rule and forge.yaml loading; dependency detection from package.json, bun.lock, pyproject.toml, uv.lock; glob→RegExp | `src/packs.ts` |
| sha256, `run()`, git helpers, version constants, `packageRoot()` | `src/util.ts` |
| All 15 packs from §13, one YAML per rule, `context.md` files, init templates (python: `ruff.toml`, `pyrightconfig.json`; typescript: `biome.json`, `tsconfig.json`) | `packs/**` |
| Hook runtime: `json_get`, `json_escape`, `tool`, `astgrep`, `repo_root` (tested on sh + dash) | `templates/scripts/lib.sh` |
| Guard (tested: every guard example passes on sh + dash) | `templates/scripts/guard.sh` |
| `check.sh` template, steps 1–4 written, stages call `forge_run_stage` from the generated fragment | `templates/scripts/check.sh` |

## Remaining

1. **Generated checks fragment** (`@@CHECKS@@` in check.sh, also embedded in post-edit.sh and typecheck.sh), rendered from active `check` targets:
   - `IFS` is newline in these scripts. Paths with spaces then work, and `forge_exec_tool` re-splits the `tool` output on spaces.
   - One wrapper function per project tool used in `run` strings (ruff, basedpyright, biome, tsc, alembic, pytest, prisma, vitest, playwright), dispatching through `tool <name>`. `uv` and `bun` are system commands, checked with `command -v`.
   - For each check `n`: `forge_check_meta_n` (sets `c_id c_stage c_tools c_when c_localdb c_timeout c_changed`), `forge_scope_n <file>` (a literal `case` built from language extensions + `scope.files`; `!` globs exclude), `forge_when_n <file>`, `forge_check_run_n`, `forge_check_changed_n` (`{files}` → `$FORGE_FILES`).
   - `forge_run_stage <stage>`:
     - Under `--changed`, skip when no in-scope file changed (or no `when_changed` match).
     - A missing tool prints `forge:<id> skipped: <tool> not installed`.
     - `requires_local_db` uses `forge_local_db` (`DATABASE_URL` env → `.env` → `alembic.ini sqlalchemy.url`; localhost/127.0.0.1/::1/empty host/socket = local). Otherwise it prints `forge:<id> skipped: no local database`.
     - Timeouts go through a background-sleep-kill helper.
     - Failure prints `forge:<id> failed` + first 15 lines and returns 1.
     - `forge:` lines from a passing run (prisma shadowDatabaseUrl skip) are echoed.
   - `forge_related_py_tests` (used by `pytest/run` changed_run): changed test files + `tests/**/test_<module>.py`, newline-separated.
2. **Hook scripts** (templates → `.claude/skills/forge-project/scripts/`): `post-edit.sh`, `typecheck.sh` (mkdir lock in `.forge/state/`, pid takeover if stale), `stop.sh` (§10.2), `session-start.sh` (≤ 20 lines, missing tools reported once via a state file), `notify.sh` (user level, OSC 777 / OSC 9 for iTerm.app and WezTerm via `terminalSequence`).
3. **Generator / plan** (pure: inputs → files map + settings entries + lock + report). It covers:
   - active packs: detect + `forge.yaml` add/remove + `requires` closure
   - local rule override, disable with reason
   - static tool presence:
     - biome: `biome.json(c)` or `@biomejs/biome`
     - ruff: `ruff.toml` / `[tool.ruff]` / dep
     - basedpyright: dep / `[tool.basedpyright]` / `pyrightconfig.json`
     - tsc: `tsconfig.json` / typescript
   - ast-grep fallback only when none of a rule's `tool` targets' tools are present
   - check targets whose tool is absent → "not enforced here"
   - `.forge/ast-grep/{sgconfig.yml, rules/<pack>/<name>.yml, rule-tests/*.yml}`
   - `guard.rules` (deny lines first, header comment naming source rule files)
   - `hooks.json`, `plugin.json`, `.claude/rules/forge-<pack>.md`, `.forge/.gitignore`
4. **Settings** (jsonc-parser `modify`/`applyEdits`, comments preserved):
   - Append only. Array targets append items; scalar targets are set only if absent.
   - Record only entries Forge actually added.
   - Confirm `y` / `--yes` for shared files.
   - On `remove`, prune containers emptied by the removal, and delete the file if it ends as `{}`.
5. **Lock** (`.forge/forge.lock.json`, `~/.claude/forge.lock.json`). Pretty-printed with 2-space indent, one `"path": "hash"` per line under `  "files": {`, because `check.sh` parses it with awk. The lock does not hash itself.
6. **CLI** (`src/cli.ts`): `init`, `apply [--user|--local] [--force] [--dry-run] [--yes]`, `diff`, `status`, `explain`, `test`, `remove`. Plus:
   - The `claude --version` gate (≥ 2.1.295).
   - The pre-commit offer (§10.3).
   - `apply` stops when an owned file is drifted, or when an unowned file sits at an owned path, unless `--force`.
7. **Audit** (§11): biome `lint --stdin-file-path`, `ruff check --stdin-filename`, temp files for basedpyright and tsc (temporary tsconfig extending the project's). Search output for `expects`.
8. **`forge test`** (§14.1):
   - schema validation
   - ast-grep rule-tests via `ast-grep test --skip-snapshot-tests`; the loaded-rule count must equal the rule-file count
   - guard examples **per rule in isolation** on sh, dash, BusyBox (skip a missing shell with a note)
   - permission examples via a small matcher: `Bash(prefix:*)` = prefix + word boundary, Read/Edit = glob
   - `context.md` ≤ 10 lines
9. **Tests**: `tests/generator/` sample repos + snapshots (§14.2, including an ESLint repo), guard corpus + p99 ≤ 20 ms with 50 rules (§14.3). E2E (§14.4) needs a container with real `claude -p` runs; write the harness, flag that it was not run.

## Deliberate departures from the spec

- **ast-grep invocation.** `bunx @ast-grep/cli@X ast-grep` fails. The working form is `bunx -p @ast-grep/cli@X ast-grep`.
- **guard.sh matching.** It uses one awk pass with the same POSIX ERE instead of one `grep -Eq` per rule, to meet the 20 ms p99.
- **guard.rules format.** It has a 4th tab-separated column holding the rule description, used for `permissionDecisionReason`.
- **TypeScript parsing.** `scope.language: typescript` compiles to ast-grep `language: Tsx`, with `languageGlobs: {tsx: ["*.ts","*.mts","*.cts"]}` in sgconfig. One rule file then covers `.ts` and `.tsx`.
- **Exclusion globs.** `scope.files` entries starting with `!` become ast-grep `ignores`. They are needed for "non-test files" rules.
- **Check timeouts.** `check` targets accept an optional `timeout` (seconds), used by `pytest/run` (300 s).
- **check.sh is self-contained.** It inlines lib.sh so CI does not depend on `.claude/`.
- **Built-in check ids.** `forge/integrity`, `forge/applied-migration`, `forge/deleted-test`, `forge/ast-grep`.
- **Pydantic detection.** `pydantic` is also detected via a `fastapi` dependency.

## Verified facts (Claude Code docs, 2026-10-20)

- **Plugin loading.** A skills-dir folder with `.claude-plugin/plugin.json` loads as a plugin (`<name>@skills-dir`, after workspace trust). It includes `hooks/hooks.json`, and `${CLAUDE_PLUGIN_ROOT}` works in its commands.
- **Hook fields.** Command hooks support `timeout`, `async`, `asyncRewake` (exit 2 wakes Claude; stderr shown), and `onFailure: "continue"|"block"` (≥ 2.1.295).
- **Notification output.** Notification matchers include `permission_prompt` and `idle_prompt`. The `terminalSequence` output accepts OSC 0/1/2/9/99/777 and is interactive-only.
- **Permission syntax.** `Bash(x:*)` is still valid. `~/` and `**` work in Read/Edit rules. Absolute paths use `//`.
- **Sandbox keys.** `sandbox.enabled` and `sandbox.network.allowedDomains`.

## Open points

- **pydantic false positives.** `pydantic/v2-api` forbids `$M.json()` and `$M.dict()` as the spec says. This also flags `response.json()` (httpx/requests, FastAPI tests).
- **Broad `.query` match.** `sqlalchemy/no-query-api` (`$S.query($$$)`) matches any `.query(` call.
- **Untested ast-grep rules.** None of the rules have been run through `ast-grep test` yet. Expect a few patterns to need fixes, for example `nextjs/proxy-not-middleware` with an empty `no_match`, and the pydantic `Config` rule.
