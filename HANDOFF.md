# Forge — hand-off notes

Build state of Forge against `SPECS.md` (v0.5), as of 2026-10-20 (third session). Read the spec first, then this file.

## Toolchain

- Bun 1.3+, Node ≥ 20, uv, Claude Code ≥ 2.1.295. BusyBox is optional (the guard matrix skips a missing shell with a note).
- Dev dependencies (`package.json`): `yaml`, `ajv` (2020-12), `jsonc-parser` (imported from `jsonc-parser/lib/esm/main.js`: its UMD entry does not bundle), `semver`, `smol-toml`, `typescript`. Run `bun install`.
- Build: `bun run build` → `dist/forge.js`; `scripts/postbuild.mjs` adds the shebang, chmods it and writes `schemas/*.schema.json`.
- Tests: `bun run test` (build + `bun test ./tests/*.test.ts`; the path glob keeps bun away from the sample apps' own `*.test.ts`). About 60 s; ast-grep runs through `bunx`, and `tests/audit.test.ts` installs Biome, TypeScript, ruff and basedpyright into a temp cache, so the first run needs the network. `FORGE_UPDATE_SNAPSHOTS=1` rewrites `tests/generator/__snapshots__/`. `bun run typecheck` covers `src/`, `tests/` and the docs code.
- Docs: VitePress in `docs/`. `bun run docs:dev` / `docs:build` (they run VitePress on the Bun runtime: jsonc-parser's ESM build has extensionless imports that Node's loader rejects). `docs/.vitepress/packs.ts` generates `docs/packs/*.md`, `docs/reference/rules.md` and `docs/reference/spec.md` from `packs/` and `SPECS.md` at every build; they are gitignored. In dev, editing a pack regenerates its page, and adding or removing a pack touches the config so VitePress reloads the sidebar.
- CI (`.github/workflows/ci.yml`): typecheck, the full test suite and `forge test` on ubuntu-latest (with dash and BusyBox) and macos-latest (bash 3.2 as `/bin/sh`), plus a docs build. `docs.yml` deploys the site to GitHub Pages on every push to main; the repository's Pages source must be set to "GitHub Actions".
- ast-grep pinned to **0.45.3**.

## Done

Everything in §3–§14 is implemented. §14.1–§14.3 and the behaviors below are covered by 199 automated tests; §14.4 has a harness that has not been run.

| Area | Files |
|---|---|
| Types, strict YAML, JSON Schemas (Ajv strict, with `strictRequired`/`strictTypes` off for the `if/then` target schemas), loaders, detection | `src/types.ts`, `src/yaml.ts`, `src/schemas.ts`, `src/packs.ts` |
| Renderers: checks fragment, guard.rules, ast-grep rules/tests/sgconfig, hooks.json, plugin.json | `src/render.ts` |
| Generator (pure): active packs (detect, forge.yaml add/remove, `requires` closure; packs requiring a removed pack are dropped too), local overrides, disable, deprecated, static tool presence, ast-grep fallback, files map, settings entries | `src/plan.ts` |
| Settings append/remove (JSONC, comments kept; only added entries recorded; scalars never overwritten, with a warning) | `src/settings.ts` |
| Lock | `src/lock.ts` |
| Audit (§11) | `src/audit.ts` |
| `forge test` (§14.1) | `src/selftest.ts` |
| CLI: all commands, `claude --version` gate, drift / unowned-file stop, `--force`, `--dry-run`, `--yes`, pre-commit offer, `remove` (also deletes `.forge/state/` and prunes empty dirs) | `src/cli.ts` |
| Check engine (static shell shared by check.sh, post-edit, typecheck, session-start) | `templates/scripts/engine.sh` |
| Hook scripts | `templates/scripts/{post-edit,typecheck,stop,session-start,notify}.sh` |
| Tests | `tests/*.test.ts` (see the conformance table), `tests/generator/{fastapi-app,nextjs-prisma-app,eslint-app}` + snapshots |
| Docs site, generated pack pages | `docs/`, `docs/.vitepress/packs.ts` |
| CI and Pages | `.github/workflows/{ci,docs}.yml` |
| E2E harness (§14.4) — **written, not run** (needs an authenticated `claude -p` and PostgreSQL) | `tests/e2e/` (see its README) |

## Remaining

1. **Run the E2E harness** (`tests/e2e/README.md`) in a real container and fix whatever it finds. The regexes in `run.ts` match Forge's own output, but how Claude Code frames hook output in the stream and debug log was not observed. Scenarios that depend on model behavior (e.g. "impossible test", three failed Stops) may need prompt tuning.
2. **macOS** is covered by the macos-latest CI job (bash 3.2 `/bin/sh`, BSD tools); check its first run.
3. **Publishing**: the npm scope is still `@maximedepachtere`; nothing was published.

## Spec conformance

| Spec | Tested by |
|---|---|
| §3 CLI: commands, `--dry-run`, `--yes`, `--force`, `--local`, exit codes, version gate | `generator`, `inputs` |
| §4 ownership, §12 lock: drift, unowned files, deleted files, missing settings entries, changed rules, remove | `generator`, `inputs`, `user` |
| §5 inputs: detection, `forge.yaml` add/remove/disable, local rules, invalid inputs (schema, duplicate keys, aliases) | `generator`, `inputs`, `units` |
| §6 targets → output; tool absent → ast-grep fallback / not enforced (ESLint repo) | `generator` snapshots |
| §7 user level: files, settings with comments and pre-existing entries, scalar conflicts, guard, notify OSC 777/9 | `user` |
| §8 project hooks: session-start, post-edit, typecheck (lock, takeover), stop (3 blocks, pass memory), missing runtime | `check` |
| §9 lib.sh (`json_get` incl. `\uXXXX`), guard format | `guard`, `user`, `units` |
| §10 check.sh steps 1–7, local-DB detection, related pytest files, pre-commit offer / hook managers / existing hook / blocks a bad commit | `check`, `inputs` |
| §11 audits with real tools, correct and incorrect configs, stricter configs, configured-but-not-installed | `audit` |
| §14.1 `forge test` on the shipped packs, and on broken packs (each failure kind) | `units`, `inputs` |
| §14.2 generator samples | `generator` |
| §14.3 guard corpus on sh, dash, bash, BusyBox; p99 ≤ 20 ms | `guard` |
| §14.4 end-to-end with real `claude -p` | `tests/e2e/` harness, **not run** |
| Docs cover every pack and rule | `docs` |

## How the generated checks work

- Every check target becomes `forge_check_meta_<n>` (`c_id c_stage c_tools c_when c_localdb c_timeout c_changed c_files`), `forge_scope_<n>` (literal `case` from the language extensions and `scope.files`; `!` globs exclude), `forge_when_<n>`, `forge_check_run_<n>` and, with `changed_run`, `forge_check_changed_<n>` (`{files}` → unquoted `$FORGE_FILES` with `IFS` = newline and `set -f`).
- Run strings are inserted verbatim. Project tools used in them (`ruff`, `basedpyright`, `biome`, `tsc`, `alembic`, `pytest`, `prisma`, `vitest`, `playwright`) are shell functions dispatching through `tool <name>`, so the strings run unchanged. `uv` and `bun` are checked with `command -v`.
- `--all` runs every check regardless of `when_changed` and scope (CI should verify everything); `--changed` skips a check with no in-scope changed file, or with `when_changed` and no matching changed path.
- A check whose audited tool (biome, ruff, basedpyright, tsc) is not configured in the project is not generated at all and is reported "not enforced here"; a check whose binary is merely not installed is generated and prints `skipped: <tool> not installed` at run time.
- Within a stage all checks run; check.sh stops after the first failing step or stage.

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
- **Biome audit uses a temporary file**, not `biome lint --stdin-file-path`: Biome 2's stdin mode echoes the content and prints no lint diagnostics. basedpyright and tsc use temporary files as specified; ruff uses stdin.
- **Hook commands** are `sh "${CLAUDE_PLUGIN_ROOT}/scripts/<x>.sh"`, so no executable bit is needed and a deleted `guard.sh` makes the hook fail (exit 127), which `onFailure: "block"` turns into a block.
- **Stop hash** also includes each untracked file's blob hash (the spec says the list of untracked files), so editing an untracked file again re-runs the check. After a pass the hash is recomputed, because `changed_run` formatters may have rewritten files.
- **Pre-commit hook** created by `apply` is recorded in the lock's `files` (`.git/hooks/pre-commit`), so `remove` deletes it and drift is detected; check.sh's integrity step only looks at `.forge/` and `.claude/` paths. It is only offered interactively or with `--yes`.
- **User-level paths** are relative to the home directory (`.claude/settings.json`, `.claude/skills/forge-user/...`, `.claude/forge.lock.json`), the same shape as project paths.
- **`python/lint` `changed_run`** is `ruff check --output-format concise {files}`: ruff's default output does not fit the 20-line PostToolUse budget.
- **Receiver constraints** (data-only changes to two rules, for the two false positives the previous hand-off flagged): `pydantic/v2-api` does not flag `.json()` / `.dict()` on `r`, `res`, `resp`, `response`, `*_response`; `sqlalchemy/no-query-api` only flags `.query()` on session-like receivers (`session`, `db`, `*_session`, `s`, `sess`, `self.session`).
- **`forge test` structure checks** beyond the schema: at most one ast-grep target per rule, no ast-grep target on `language: any`, and match + no_match examples on every rule with ast-grep, guard, permission or tool targets. Guard examples also run under `bash --posix`.
- **Deprecated rules** are not enforced; `status` lists them with their replacement.

## Verified facts (Claude Code docs, 2026-10-20)

- **Plugin loading.** A skills-dir folder with `.claude-plugin/plugin.json` loads as a plugin (`<name>@skills-dir`, after workspace trust). It includes `hooks/hooks.json`, and `${CLAUDE_PLUGIN_ROOT}` works in its commands.
- **Hook fields.** Command hooks support `timeout`, `async`, `asyncRewake` (exit 2 wakes Claude; stderr shown), and `onFailure: "continue"|"block"` (≥ 2.1.295).
- **Notification output.** Notification matchers include `permission_prompt` and `idle_prompt`. The `terminalSequence` output accepts OSC 0/1/2/9/99/777 and is interactive-only.
- **Permission syntax.** `Bash(x:*)` is still valid. `~/` and `**` work in Read/Edit rules. Absolute paths use `//`.
- **Sandbox keys.** `sandbox.enabled` and `sandbox.network.allowedDomains`.

## Open points

- **Settings formatting.** jsonc-parser reformats the arrays/objects Forge appends to (inline `["a"]` becomes multi-line). Values and comments are preserved and `remove` restores the values, but not necessarily the original layout of a pre-existing user file. A file Forge created is deleted on `remove`, so project repos come back clean.
- **ast-grep on `.ts` with the Tsx parser**: old-style `<T>value` casts in `.ts` files misparse. Rare in modern code; kept for the one-rule-file design.
- **Missing-tool noise.** check.sh prints `forge:<id> skipped: <tool> not installed` on every run, including CI. Intended (visible, never blocking), but could be reduced to once per run in CI if it proves noisy.
