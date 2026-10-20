# Forge — hand-off notes

Build state of Forge against `SPECS.md` (v0.5), as of 2026-10-20 (fourth session). Read the spec first, then this file.

## Toolchain

- Bun 1.4, Node ≥ 20, uv, Claude Code ≥ 2.1.295. dash and BusyBox are optional locally (the shell matrix skips a missing shell).
- `bun install`. `jsonc-parser` is imported from `jsonc-parser/lib/esm/main.js` (its UMD entry does not bundle; its ESM build has extensionless imports, which is also why the docs run Astro on the Bun runtime).
- Build: `bun run build` → `dist/forge.js`; `scripts/postbuild.mjs` adds the shebang, chmods, and writes `schemas/*.schema.json`.
- **Spec / tests:** `bun run test` runs `tests/spec/*.test.ts` (≈ 80 s; ast-grep via `bunx`, and `11-audits` installs Biome, TypeScript, ruff and basedpyright into a temp cache, so the first run needs the network). `bun run test:report` does the same and writes `test-results/<platform>.xml` (JUnit). `FORGE_UPDATE_SNAPSHOTS=1` rewrites `tests/generator/__snapshots__/`. `bun run typecheck` covers `src/`, `tests/` and `docs/src/lib/spec.ts`.
- **Docs:** Astro 7 + Starlight 0.42 + React 19 in `docs/` (`bun run docs:dev`, `docs:build`; output `docs/dist/`). No Vue anywhere. `astro preview` does not start under Bun; serve `docs/dist` with any static server under `/claude-forge/` instead.
- **CI:** `.github/workflows/ci.yml` runs typecheck and `test:report` on ubuntu (with dash, BusyBox) and macOS (bash 3.2 as `/bin/sh`), uploads the JUnit files, and builds the docs with them. `docs.yml` runs after each CI run on `main`, downloads that run's results, checks out that run's commit, builds and deploys to GitHub Pages. The repository's Pages source must be "GitHub Actions" and Actions must be enabled.
- ast-grep pinned to **0.45.3**.

## Done

Everything in §3–§14 is implemented, with the pack tree (§5.1–§5.2) and the Astro/React packs added to the spec in this session. §14.1–§14.3 and everything else listed in the spec site are covered by **356 tests**; §14.4 has a harness that has not been run.

| Area | Files |
|---|---|
| Types, strict YAML, JSON Schemas, **pack tree loader** (`loadPacks` walks nested directories; ids are paths; `lineage()`), detection | `src/types.ts`, `src/yaml.ts`, `src/schemas.ts`, `src/packs.ts` |
| Generator: activation down the tree (child only under an active parent; `add` brings ancestors; `remove` drops the subtree), local overrides, disable, tool presence, ast-grep fallback | `src/plan.ts` |
| Renderers, settings (JSONC), lock, audit, `forge test`, CLI (`USAGE` in `src/usage.ts`) | `src/render.ts`, `src/settings.ts`, `src/lock.ts`, `src/audit.ts`, `src/selftest.ts`, `src/cli.ts` |
| Hook runtime and check engine | `templates/scripts/*.sh` |
| Packs: 18 packs, 61 rules, as a tree (`safety`; `python/{uv,fastapi,pydantic,pytest,sqlalchemy/alembic}`; `typescript/{bun,nextjs/react,astro/react,prisma,zod,vitest,playwright}`) | `packs/**` |
| **Executable specification**: one file per spec section, normative test titles | `tests/spec/*.test.ts`, `tests/helpers.ts`, samples in `tests/generator/{fastapi-app,nextjs-prisma-app,astro-app,eslint-app}` |
| **Docs site**: spec pages rendered from the test files + JUnit results (build fails on an unplaceable result); pack tree, pack and rule pages from `packs/`; CLI page from `USAGE`; schema page from `src/schemas.ts`; four short hand-written guide pages that link into the spec | `docs/` (`src/lib/spec.ts` extractor, `src/lib/data.ts`, `src/pages/**`, `src/components/{SpecNodes,RuleCard}.astro`, `{PackTree,StatusBadge}.tsx`) |
| E2E harness (§14.4) — **written, not run** | `tests/e2e/` |

## Remaining

1. **Run the E2E harness** (`tests/e2e/README.md`) in a real container; expect to tune a few regexes and prompts on the first real run.
2. **First CI run.** Nothing has run on GitHub yet (Actions did not start on push, and this session could not dispatch workflows). Watch the macOS job in particular.
3. **More packs** for the tree, e.g. React rules under Astro (`.astro` files cannot be parsed by ast-grep; rules there would be `check` or `tool` targets).
4. **Publishing**: the npm scope is still `@maximedepachtere`; nothing was published.

## Writing specification tests

- A file in `tests/spec/` starts with `/** @spec <number> · <title>` and a Markdown paragraph; the site orders sections by file name.
- Test titles are requirements, in the present tense. A `/** … */` comment right above a `test` or `describe` is rendered as explanation.
- The site shows each test's code, its parameterised cases and its result per platform, matched by file and **title line** (that is the line bun's JUnit reporter gives).
- A test that needs a tool which may be missing uses `test.if(...)`, so the site shows it as skipped rather than passed.

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
