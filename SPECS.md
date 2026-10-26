# Forge — Build Specification v0.5

Forge is a command-line generator. It reads rules grouped in packs and writes a standalone Claude Code setup at user level (`~/.claude/`) or project level (a git repository). Once written, the setup runs with plain Claude Code; no generated file calls Forge.

Every requirement uses MUST / MUST NOT. Anything not described here is out of scope.

---

## 1. Principles

1. **Standalone output.** Generated files never call Forge.
2. **Nothing to install.** Every hook is POSIX `sh` with `grep`, `sed`, `awk` and `git`. Project tools are the project's own binaries. Forge-only tools (ast-grep) run through pinned `bunx` / `uvx`.
3. **Forge writes only what it owns.** Files the project owns (tool configs, `package.json`, dependencies, git config, `CLAUDE.md`) are audited, never written.
4. **Rules are data.** One YAML file per rule, with examples that double as tests. Adding a rule means adding a file.
5. **Enforce in the closest native place:** the project's own tools, then ast-grep, then Claude Code permissions and sandbox, then hooks. Hooks are only for what concerns Claude's actions.
6. **Claude cannot change the setup unnoticed.** Forge-owned files are behind permission `ask` and are hash-checked by `check`. Git is the only history.
7. **Success is silent.** Every message starts with `forge:<rule-id>`.

## 2. Stack and platform

| Slot | Python | TypeScript |
|---|---|---|
| Package manager | uv | Bun |
| Framework | FastAPI | Next.js 16 (App Router) + React 19; Astro (+ React islands) |
| Data | SQLAlchemy 2.0 async, typed declarative + Alembic async | Prisma 7 |
| Validation | Pydantic v2 | Zod |
| Format + lint | ruff | Biome 2 |
| Typecheck | ty (Forge's pinned ty, §15.2) | TypeScript 7 `tsc` |
| Unit tests | pytest | Vitest |
| E2E tests | — | Playwright |
| Database | PostgreSQL | PostgreSQL |

- One application per repository.
- Platforms: macOS, Linux, WSL2.
- Claude Code ≥ 2.1.295. `forge apply` reads `claude --version` and refuses below it.
- A project that uses other tools is never broken: rules whose tool is absent are reported as "not enforced here" (§11).

## 3. Forge CLI

- TypeScript, ESM, bundled with `bun build --target=node` into `dist/forge.js` (`#!/usr/bin/env node`). Runs on Node ≥ 20 and Bun. No Bun-only APIs.
- Published as `@<npm-scope>/forge`; binary `forge`. Run with `bunx` or `npx`.
- Packs ship inside the package. The build loads and validates them and writes the result to `dist/packs.json`, which the CLI reads (a pack or rule file that fails validation fails the build).
- Bundled libraries: YAML parser rejecting duplicate keys, anchors, aliases and custom tags; JSON Schema 2020-12 validator; JSONC reader/writer preserving comments (for `settings.json`).

### 3.1 Commands

| Command | Behavior |
|---|---|
| `forge init` | New project only. Creates the stack's tool configs from pack templates (§5.2), each only if the file does not exist, then runs `forge apply`. |
| `forge apply` | Generate or update the project-level setup in the current repository. |
| `forge apply --user` | Generate or update the user-level setup. |
| `forge apply --local` | Like `apply`, but permission entries go to `.claude/settings.local.json` (for repositories you don't own). |
| `forge diff [--user]` | Show what `apply` would change. Exit 1 if anything differs. |
| `forge status [--user] [--json]` | Active packs; rules as `Rules: <n> active, <m> enforced, <k> not enforced (<count> <reason>, …)` then each rule with where it is enforced; disabled rules with reasons; audit results with snippets; drift; missing tools. A rule is enforced when at least one of its targets acts here: a satisfied audit, an ast-grep rule, a permission, guard, sandbox or grep target, or a check whose tools are installed. Reasons: `ruff codes not selected`, `Biome rules not at error`, `ty rules not at error`, `tsc options not set` (unsatisfied audits), `<tool> not configured`, `<tool> not installed`. Missing tools are what the generated scripts' `forge_missing_tools` prints (the SessionStart hook reports the same list). `--json` prints all of it as one object: `counts`, `rules` (`id`, `enforced`, `by`, `not_enforced`), `audits`, `missing_tools`, `packs`, `disabled`, `drift`. |
| `forge explain <rule-id>` | The rule file, where it is enforced in this project, and its audit result. |
| `forge test` | Inside a git repository: validate the project's local rules (`.forge/rules/`; a local rule replacing a pack rule is tested as written) against the schema and run their examples (§14.1). No local rules: prints that there is nothing to test, exit 0. The bundled packs are not run: they are proved by Forge's own test suite. |
| `forge remove [--user]` | Delete every owned file and exactly the settings entries recorded in the lock. |

- Every writing command accepts `--dry-run` (prints the `diff` output, writes nothing).
- `apply` is idempotent. If an owned file's hash differs from the lock, `apply` stops and prints the diff unless `--force` is given.
- Writes to a shared settings file are shown as a diff and require `y` confirmation (`--yes` for non-interactive use).
- `apply --pre-commit` consents to adding the commit-time check to a hook manager's config (§10.3); `--yes` does not, because that file is the team's and usually tracked (a script or CI job running `apply --yes` must not edit it).

## 4. Ownership

| Category | Paths | Forge does |
|---|---|---|
| **Owned** | `.forge/**`, `.claude/skills/forge-project/**`, `.claude/rules/forge-*.md`; user level: `~/.claude/skills/forge-user/**`, `~/.claude/forge.lock.json` | Regenerates; hashes recorded in the lock |
| **Appended** | `.claude/settings.json` (or `.claude/settings.local.json` with `--local`); `~/.claude/settings.json` | Adds entries only, with consent; each entry recorded in the lock; never changes or removes entries it did not add |
| **Audited** | `biome.json`, `tsconfig.json`, `pyproject.toml`, `package.json`, dependencies | Reads and tests behavior (§11); never writes |
| **Marked block** | a hook manager's config: `.pre-commit-config.yaml`, `lefthook.yml` (`.yaml`, `.lefthook.yml`), `.husky/pre-commit`, `<core.hooksPath>/pre-commit` (§10.3) | Adds one block between markers, only with `--pre-commit` or a yes at the prompt (never with `--yes` alone); recorded in the lock's `blocks`; `remove` takes it out; never changes the rest of the file |
| **Untouched** | `CLAUDE.md`, other `.claude/rules/*`, other `.claude/skills/*`, git config, `.git/hooks/*` (except `.git/hooks/pre-commit`, §10.3), the rest of the hook managers' files, CI files | Nothing |

`forge init` is the only command that creates audited files, and only when they don't exist.

## 5. Inputs

### 5.1 Packs

Packs form a tree: **language first, then package, then sub-package** (`typescript` → `typescript/nextjs` → `typescript/nextjs/react`). The same package can appear under several parents with different rules (`typescript/nextjs/react`, `typescript/astro/react`). Packs that are not tied to a language are roots of their own: `safety` (user level), `docs` (project level).

`packs/<path>/pack.yaml`:

```yaml
id: string                  # the pack's path in the tree: ^seg(/seg)*$, seg = ^[a-z][a-z0-9-]*$
description: string
level: user | project       # equal to the parent's level
requires: [pack-id]         # other branches only; the parent is implied
detect:                     # omitted for user-level packs
  files_any: [glob]
  deps_any: [{ecosystem: pypi | npm, name: string, version: semver-range}]
  library_any: [pypi | npm]  # the project is a library published there (§13.4)
```

- A root pack is active when detected; a child pack is active when its parent is active **and** it is detected.
- `forge.yaml` `packs.add` activates a pack and its ancestors. `requires` activates the required pack and its ancestors.
- `packs.remove` deactivates a pack, its whole subtree, and every pack that requires a removed pack.
- Detection reads `pyproject.toml`, `uv.lock`, `package.json`, `bun.lock`.
- **Workspaces.** Detection also reads the workspace packages of a monorepo: the directories (holding a `package.json` or `pyproject.toml`) matched by `pnpm-workspace.yaml` `packages`, `package.json` `workspaces` (array, or `{packages}`), `[tool.uv.workspace] members`, or `lerna.json` `packages` (default `packages/*`), minus `!` patterns and uv `exclude` (an exclusion like `**/test/**` also excludes the `test` directory itself); without any declaration, the repository has no workspace packages. Globs support `**`, `*`, `?` and `{a,b}` alternatives, which may nest and hold `**`. The file list is read once and bucketed by package. A pack is detected when it is detected at the root or in one package, each judged on its own dependencies and files (versions are never merged across packages; `files_any` globs match relative to each package, and the root's files are those outside the packages). The root's lock files resolve versions.
- **Where checks run.** A `check` target that runs a project tool (and no engine helper other than `forge_exec_tool`, which runs a project tool whose name has a `-`, e.g. `vue-tsc`) runs at the root when its pack is detected at the root and the root configures its audited tools (as without workspaces). Otherwise it runs in each workspace package that has its pack (any package, for a pack active at the root) and configures those tools (a config file, or a dependency when no config file is at the root); `status` shows `(in <dirs>)`. There, `{files}` gets the package's changed files relative to it, the tool is the package's own or the root's (hoisted), and relative paths in the output are prefixed with the package directory, so diagnostics are attributed as at the root. Its `format_stdin` filter runs in the package holding the file, on the path relative to it. Tool audits run in every package that configures the tool (`(in <dir>)` in the audit line), and a rule counts as enforced by the tool only when every package's config enforces it.
- **Where other rules apply.** A pack detected only in some workspace packages (not at the root, not added in `forge.yaml` or required) scopes its ast-grep rule files (`files` / `ignores`) and grep rules to those packages, its globs relative to each. Its guard, permission and sandbox entries and its context file apply to the whole repository, and helper checks (ratchet, migrations, Docker, test integrity) run from the root with globs relative to it. Package directories with spaces are not read.

### 5.2 Pack directory

```
packs/<path>/
├── pack.yaml
├── rules/<name>.yaml     # rule id: <path>/<name>
├── context.md            # optional, ≤ 10 lines of facts
├── init/                 # optional, templates used only by forge init
└── <child>/              # optional child packs (any directory with a pack.yaml)
```

`rules` and `init` are reserved names; any other sub-directory must be a pack.

### 5.3 Project inputs

```yaml
# <repo>/forge.yaml (optional)
forge: "0.5"
packs:
  add: [pack-id]
  remove: [pack-id]
disable:
  - id: <rule-id>
    reason: string          # required
accept:                     # config-ratchet findings accepted on purpose (§10.4)
  - rule: <rule-id>
    change: string          # the finding line exactly as check.sh printed it
    reason: string          # required
```

`<repo>/.forge/rules/<name>.yaml`: project-local rules. A local rule whose `id` equals a pack rule's `id` replaces it entirely. `forge.yaml` and `.forge/rules/` are inputs, not owned output: `apply` never rewrites them, but they are behind permission `ask` (§8.3).

## 6. Rules

### 6.1 Rule file

```yaml
id: <pack-id>/<kebab-name>     # stable, never reused
description: string            # one sentence
scope:
  language: python | typescript | tsx | yaml | any
  files: [glob]                # default: all files of the language
enforce:                       # ordered; see §6.2
  - target: <target>
    ...fields
examples:
  match: [example]             # MUST trigger the rule
  no_match: [example]          # MUST NOT trigger the rule
  deps: [{ecosystem, name, version}]  # rules with a where (§15.1): the packages, exact versions, the examples are type-checked against
deprecated:                    # optional
  replaced_by: <rule-id>
review: string                 # optional, one line: for an ask, the one question a reviewer answers (§9.3)
```

An example is a string (code, written to a file inside the rule's scope) or, for rules whose targets are all `check`, a **fixture**:

```yaml
- head: {<path>: <content>}    # optional: committed first
  files: {<path>: <content>}   # then written on top: the change being checked
```

A `guard` example is a command string or `{command, files}`: the command runs from a directory holding `files` (e.g. `node_modules/.bin/tsc`), which is its repository root.

Paths are relative, without `..`. Every rule has examples, except a `check` target that runs the project's tests (stage `test`), its migrations (stage `migration`), `astro check` or a lock-file check (`uv`, `bun`): those need a whole project, and their examples are optional.

Every rule blocks, except an `ast-grep` target in `advise` mode (§6.2), for heuristics whose matches need judgement. A rule you don't want is disabled with a reason.

### 6.2 Targets

| Target | Fields | Category | Effect |
|---|---|---|---|
| `tool` | `tool: biome \| ruff \| ty \| tsc`, `expects` (diagnostic code or rule name), `snippet` (config to show when unsatisfied) | audited | §11 |
| `ast-grep` | `rule` (ast-grep rule object), `mode: forbid \| no-increase \| no-decrease \| advise`, `constraints`, `utils` (passed through), `where` (optional: engine capabilities on metavariables, §15.1) | owned | `.forge/ast-grep/rules/<rule-id>.yml` |
| `permission` | `decision: deny \| ask`, `rule` (permission rule string) | appended | settings `permissions` |
| `guard` | `decision: deny \| ask`, `pattern` (POSIX ERE against the JSON-escaped command), `check` (optional, `decision: ask` only: a built-in guard check, `ephemeral-exec`, that decides when `pattern` matches) | owned | `guard.rules` (§9.2) |
| `sandbox` | `path`, `value` | appended, user only | `~/.claude/settings.json` `sandbox` |
| `check` | `stage: format \| lint \| typecheck \| migration \| test`, `run`, `changed_run` (optional, `{files}`), `fix_run` (optional, lint only, `{files}`: safe fixes applied after an edit, §8.1), `format_stdin` (optional, format or lint, `{file}`: the formatter as a filter, formatting stdin as if it were `{file}`; formats base copies for added lines, §10.1), `rerun` (optional, test only, `{files}`: runs only the given test files, §10.6), `when_changed` (optional), `requires_local_db` (optional, boolean) | owned | `.forge/check.sh` (§10) |
| `grep` | `pattern` (POSIX ERE, matched line by line), `mode: forbid \| no-increase \| no-decrease`, `ignore_case` (optional) | owned | `.forge/check.sh` (§10): counts are matching lines per file |

ast-grep and grep modes:
- `forbid`: a match on an added line of a changed file fails (§10.1); with `--all`, any match.
- `no-increase`: more matches than in the base in a changed file fails.
- `no-decrease`: fewer matches than in the base in a changed file fails.
- `advise` (ast-grep only): a match on a line added during the turn is reported after the edit (PostToolUse feedback, like `forbid`); Stop, the pre-commit hook and CI do not run it.

The schema rejects fields that would do nothing: `rerun` outside stage `test`, `format_stdin` outside `format` / `lint`, `check` on a guard with `decision: deny`, and `review` on a rule with neither a guard `ask` nor the test-edit check (`run: forge_test_edits`). A project tool whose name has a `-` (`vue-tsc`) has no wrapper function, so a check runs it as `forge_exec_tool <name>`; a rule running it directly is rejected when packs and local rules load.

A `grep` target is for text that is not code ast-grep parses: comments, configuration, `.astro` files.

`scope.language` picks the ast-grep parser and the files a check covers: `python` (`*.py`, `*.pyi`), `typescript` (`.ts`, `.tsx`, `.mts`, `.cts` and the JavaScript extensions; ast-grep `Tsx`), `tsx` (`*.tsx`, `*.jsx`), `yaml` (`*.yaml`, `*.yml`; ast-grep `Yaml`). In YAML rules, a key is matched on the `key` field of a `block_mapping_pair` with a regex that allows quotes (`^["']?name["']?$`), several `has` clauses go under `all:`, and scalars are `string_scalar`, `double_quote_scalar`, `single_quote_scalar` or `integer_scalar` (quoted kinds include their quotes).

A `tool` target is followed by an `ast-grep` target when the rule must still apply in projects that use a different tool for that job. The `ast-grep` target is used only when the `tool` target's tool is absent from the project.

### 6.3 Example

```yaml
# packs/typescript/rules/no-explicit-any.yaml
id: typescript/no-explicit-any
description: No explicit any. Use unknown with narrowing, or a Zod schema.
scope: {language: typescript}
enforce:
  - target: tool
    tool: biome
    expects: lint/suspicious/noExplicitAny
    snippet: '"linter": {"rules": {"suspicious": {"noExplicitAny": "error"}}}'
  - target: ast-grep
    mode: forbid
    rule: {any: [{pattern: "$A as any"}, {kind: predefined_type, regex: "^any$"}]}
examples:
  match: ["const x = y as any", "function f(a: any) {}"]
  no_match: ["const x: unknown = y", "const many = 1"]
```

## 7. Generated output — user level

```
~/.claude/
├── settings.json                         # appended entries only
├── skills/forge-user/
│   ├── .claude-plugin/plugin.json        # {"name":"forge-user", ...}
│   ├── hooks/hooks.json
│   └── scripts/{guard.sh, guard.rules, notify.sh, review.sh, lib.sh}
└── forge.lock.json
```

- **Hooks** (`hooks/hooks.json`, commands use `${CLAUDE_PLUGIN_ROOT}`):

| Event | Matcher | Script | Options |
|---|---|---|---|
| PreToolUse | `Bash` | `guard.sh` | timeout 180 s (an ask may wait for a review, §9.3), `onFailure: "block"` |
| PermissionRequest | — | `review.sh` | timeout 180 s (§9.3) |
| Notification | `permission_prompt\|idle_prompt` | `notify.sh` | prints `{"terminalSequence":"\u001b]777;notify;Claude Code;<msg>\u0007"}`; OSC 9 when `TERM_PROGRAM` is `iTerm.app` or `WezTerm` |

- **Appended to `~/.claude/settings.json`:** the `safety` pack's `permission` and `sandbox` targets, and `ask` on `Edit(~/.claude/skills/forge-user/**)` and `Edit(~/.claude/forge.lock.json)`.
- **A sandbox that cannot start.** On Linux, when the setup enables the sandbox, `apply --user` and `status --user` check that bubblewrap can run it (`bwrap` and `socat` on PATH; `bwrap --ro-bind / / --dev /dev --proc /proc --unshare-all --die-with-parent true` succeeds; `--proc` because Claude Code's sandbox mounts a new proc). Otherwise they print why (missing packages, or bubblewrap's error: in a container with Docker's default seccomp/AppArmor profile, "Creating new namespace failed: Operation not permitted"; with Docker's masked `/proc`, "Can't mount proc on /newroot/proc: Operation not permitted", which `--security-opt systempaths=unconfined` lifts) and the ways out: give the container what bubblewrap needs, or set `"sandbox": {"enabled": false}` in `~/.claude/settings.json` knowingly (Forge never changes a value the user set).

## 8. Generated output — project level

```
<repo>/
├── .claude/
│   ├── settings.json                     # appended entries only
│   ├── skills/forge-project/
│   │   ├── .claude-plugin/plugin.json    # {"name":"forge-project", ...}
│   │   ├── hooks/hooks.json
│   │   └── scripts/{lib.sh, session-start.sh, turn-start.sh, guard.sh, guard.rules, review.sh, bash-snapshot.sh, test-edit.sh, post-edit.sh, typecheck.sh, stop.sh}
│   └── rules/forge-<pack-id>.md          # one per active pack with context.md; "/" in the id becomes "-"
└── .forge/
    ├── check.sh
    ├── ast-grep/{sgconfig.yml, rules/*.yml, rule-tests/*.yml}
    ├── helpers/                          # scripts engine helpers and the types engines run (§13.4, §15.2), when used
    ├── forge.lock.json                   # for the CLI
    ├── manifest.sha256                   # owned-file hashes for check.sh (sha256sum format)
    ├── .gitignore                        # state/
    └── state/                            # not committed: tools' caches (tsc's build info)

~/.claude/forge-state/<id>/               # the hooks' state for this repository (<state> below)
```

**Hook state.** The hooks keep their state (turn snapshots, Stop results and counters, the saved prompt, review verdicts and their log, Bash snapshots, the typecheck lock) in `<state>` = `$FORGE_STATE_HOME/<id>` (`FORGE_STATE_HOME` defaults to `~/.claude/forge-state`; `<id>` is the first 16 hex digits of the sha256 of the repository root's physical path), created readable by the user only. It is outside the repository, so the agent's Bash, which the sandbox confines to the repository, cannot forge a verdict, the request a reviewer reads, an approval or a Stop result; `safety/forge-state` denies Bash commands naming it and `safety/forge-state-edit` denies `Edit` there (§13). `.forge/state/` keeps only tools' caches.

### 8.1 Hooks

All scripts are POSIX `sh`. They read file paths and the working directory from the hook input (`cwd`, `tool_input.file_path`), never from `CLAUDE_PROJECT_DIR`.

| Event | Matcher | Script | Mode | Timeout | Behavior |
|---|---|---|---|---|---|
| SessionStart | `startup\|resume\|clear\|compact` | `session-start.sh` | sync | 10 s | ≤ 20 lines: branch, number of changed files and first 10 paths, last 5 commits (`%h %s`, ≤ 72 chars), active packs, missing tools (once). Starts the types engines the active rules use, in the background (§15.3). |
| UserPromptSubmit | — | `turn-start.sh` | sync | 10 s | Snapshots the tree at the start of the turn, resets the Stop block counter (§10.2) and saves the prompt to `<state>/<session_id>.prompt` (for a review of test edits, §10.5). Prints nothing. |
| PreToolUse | `Bash` | `guard.sh` | sync | 180 s | Project `guard` rules. `onFailure: "block"`. An ask nobody can answer is reviewed (§9.3). |
| PreToolUse | `Bash` | `bash-snapshot.sh` | sync | 15 s | Snapshots the working tree for the files the command changes (§8.2). Prints nothing. |
| PreToolUse | `Edit\|Write` | `test-edit.sh` | sync | 180 s | Asks before an edit changes or removes a test that existed at the start of the turn (§10.5); reviewed when nobody can answer (§9.3). |
| PermissionRequest | — | `review.sh` | sync | 180 s | §9.3. Defers to the user-level `review.sh` when it exists. |
| PostToolUse | `Edit\|Write\|Bash` | `post-edit.sh` | sync | 60 s | On the edited file (Bash: the files the command changed, §8.2): format silently; apply the `fix_run` safe fixes of in-scope lint checks, and when they changed the file add `forge: auto-fixed <codes> in <file>; re-read it before editing it again` (codes from ruff's `--show-fixes`, per file, else "safe lint fixes"); lint it (a lint `changed_run` that writes, like Biome's `check --write`, applies its safe fixes in the same run: a change beyond what `format_stdin` explains adds the same line); run ast-grep `forbid` rules on it. Each tool runs once with all the files in its scope. Added lines are computed after these writes. The checks get `FORGE_EDIT_BUDGET` seconds (45, under the 60 s timeout); past it they are stopped and the findings so far are returned after `forge: the checks after this edit stopped after <n>s; findings so far are below, and the Stop check covers the rest.` Findings on lines added during the turn (against the turn snapshot; before the first prompt, against `HEAD`) → JSON `{"hookSpecificOutput":{"hookEventName":"PostToolUse","additionalContext":"…"}}`, ≤ 20 lines, diagnostics for these files only. Nothing on success. |
| PostToolUse | `Edit\|Write` | `test-edit.sh` | sync | 10 s | The call ran, so its test edits were approved: they go to `<snapshot>/tests-approved`; the file goes to `<snapshot>/touched` (§10.5). Prints nothing. |
| PostToolUse | `Edit\|Write\|Bash` | `typecheck.sh` | `asyncRewake: true` | 300 s | Project typecheck, when the edited file (Bash: a file the command changed) is in a typecheck's scope. Errors in files changed during the turn (the Stop hook's change set, §10.2) → exit 2, stderr ≤ 20 lines, those files' diagnostics only; errors only in other files → silent. Skips if one is already running (lock in `<state>`, taken over when its pid is gone, or when it has had no pid for a minute). Does not run where Claude Code would wait for it: Claude Code runs an `asyncRewake` hook in the background only in an interactive session or with streaming input, so under `claude -p "<prompt>"` (`CLAUDE_CODE_ENTRYPOINT=sdk-cli` and no ancestor process with `--input-format stream-json` or `--sdk-url`; the ancestors are looked up once per session, the answer kept in `<state>/<session_id>.rewake`) it exits 0 at once and the Stop check runs the typecheck; `FORGE_EDIT_TYPECHECK=always` runs it anyway, `never` skips it everywhere. |
| PostToolUseFailure | `Bash` | `post-edit.sh`, `typecheck.sh` | as above | as above | The same two hooks for a Bash command that fails or is interrupted: it may have changed files before stopping (a script that writes three files then fails on the fourth). Same checks and output, with `hookEventName` `PostToolUseFailure`. A failed Edit or Write changes nothing, so they are not matched. |
| Stop | — | `stop.sh` | sync | 600 s | §10.2. |
| SessionEnd | — | `session-end.sh` | sync | 10 s | Only when an active rule uses the types engine (§15.3): unregisters the session; the last session to end stops the engines. Prints nothing. |

### 8.2 Files changed by Bash

Agents also change files through Bash (heredocs, `sed -i`, `cat >`, `git apply`), so the per-edit checks also follow Bash commands:

1. **Before** (`bash-snapshot.sh`, PreToolUse `Bash`): in a git repository with `.forge/`, write the working tree's git tree id to `<state>/bash/<tool_use_id>.tree`. The tree comes from `git add -A . && git write-tree` with `GIT_INDEX_FILE` set to a copy of the repository's index, `GIT_OBJECT_DIRECTORY` set to `<state>/bash/objects` and the repository's objects as `GIT_ALTERNATE_OBJECT_DIRECTORIES`: the user's index and object store are never written. Tracked and untracked files count; ignored ones do not. A snapshot that takes longer than `FORGE_SNAPSHOT_TIMEOUT` (5 s) is recorded as `skipped`.
2. **After** (`post-edit.sh`, PostToolUse `Bash`, or PostToolUseFailure `Bash` when the command failed or was interrupted): snapshot again and list the added and modified files (`git diff-tree -r --no-renames --name-status`), outside `.forge/`, `.claude/`, `.git/`, `node_modules/` and `.venv/`. The list goes to `<tool_use_id>.files` for typecheck.sh; the files (the first 20, with a note when there are more) get the same checks as an edited file, and the findings one `additionalContext` of at most 20 lines. Formatting happens after the comparison, so its writes are not counted (the next command's snapshot starts from them). A `skipped` snapshot prints, once per repository, that the Stop check covers Bash changes there.
3. **typecheck.sh** (PostToolUse or PostToolUseFailure `Bash`, async) waits up to 60 s for that list, then runs when a listed file is in a typecheck's scope.
4. **Cleanup.** A command denied before it runs never reaches PostToolUse: each snapshot drops `.tree` and `.files` older than a day or beyond the 50 newest, and the object directory once no snapshot is pending.

Not covered: writes by a `run_in_background` command after it returns, and anything outside the repository; the Stop check still sees them.

### 8.3 Permissions appended

`ask`: `Edit(.forge/**)`, `Edit(forge.yaml)`, `Edit(.claude/skills/forge-project/**)`, `Edit(.claude/rules/forge-*.md)`, plus `permission` targets of active project packs.

### 8.4 Context

- Each active pack with a `context.md` produces `.claude/rules/forge-<pack-id>.md` (`/` replaced by `-`): one line, "These are this project's defaults. When the task or the user explicitly asks for something different, do what they ask and point out the conflict; never override an explicit instruction with these defaults.", a blank line, then the same content; no `paths:` frontmatter.
- `context.md` is ≤ 10 lines, written as facts. A line never claims more than the checks enforce: a requirement names what is rejected, scoped as the check is ("`text()` with an f-string is rejected", not "SQL is always constant"), and leaves room for what the check allows.
- Context never enforces anything; enforcement is done by checks and hooks.

## 9. Shared script conventions

### 9.1 `lib.sh`

- `json_get <key>`: extracts a top-level or `tool_input` string value from stdin JSON with `grep -oE` and `sed`, unescaping `\"` and `\\`.
- `json_escape`: escapes `\`, `"`, tab, newlines and every other control character (`\u00XX`; carriage returns are dropped) with `awk` for embedding output in JSON: one raw control character makes the whole hook output invalid, and a Stop block is then lost.
- `tool <name>`: in a `bun.lock` repo, `bun run --bun <name>` if `node_modules/.bin/<name>` exists; in a `uv.lock` repo, `.venv/bin/<name>` if it exists; otherwise prints nothing (tool absent).
- `astgrep`: `bunx @ast-grep/cli@<pinned> ast-grep` in a `bun.lock` repo (run from bunx's cache with `--no-install`, installed first when missing: `bunx -p` alone installs the package again at every run), otherwise `uvx --from ast-grep-cli==<pinned> ast-grep`; on network failure, retries with `uvx --offline`. Always invoked as `ast-grep`, never `sg`.
- Only POSIX features: `[[:space:]]` (no `\s`, `\b`, `-P`), `printf '%s'` (no `echo` for data), no `sed -i`.

### 9.2 Guard

- `guard.rules`: one rule per line, tab-separated `id`, `decision`, `pattern`, `description`, then `check` and `review` (the rule's review question) when the rule has them. Deny lines first. A header comment names the source rule files.
- `guard.sh`: extracts `tool_input.command` (still JSON-escaped), tests each pattern with `awk`, prints on the first deny (else first ask) match `{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"<decision>","permissionDecisionReason":"forge:<id> <description>"}}`, exits 0. No match: prints nothing.
- **Checks.** A rule with a `check` is only a candidate when its pattern matches: the check (`guard_check_<name>` in `guard.sh`) parses the command and decides; when it does not flag it, the next matching rule answers. A check that flags the command appends its finding to the reason (`… Here: <finding>.`). Commands are split on `;`, `&&`, `||`, `|`, `&`, newlines, subshells and `$(…)`; quoted text, comments and heredoc bodies are not commands; `cd` / `pushd` move the directory later commands run in (a target with `$`, `` ` `` or a glob cannot be followed); `sh|bash|zsh -c '…'` is parsed too.
- **`ephemeral-exec`** flags a package launcher that would fetch from a registry: `pnpm dlx`, `pnpx`, `yarn dlx` and `pipx run` always; `npx`, `bunx`, `bun x`, `npm exec` / `npm x`, `uvx` and `uv tool run` when given a package option before the command name (`-p`, `--package`, `-c`, `--call`; uv's `--from`, `--with*`, index options), a spec that is not a bare name (`name@version`, `name==version`, a URL, `user/repo`), or a command not installed where it runs: `node_modules/.bin/<name>` (or `node_modules/<name>/package.json` for a package name) for the Node launchers, `.venv/bin/<name>` for uv, from that directory up to the repository root. Only options before the command name belong to the launcher (`npx tsc --noEmit -p tsconfig.json` runs tsc's `-p`); `npm exec` also reads its options after the name, up to a `--`, unless `--` precedes it. A package manager's own options before its subcommand (`npm --yes exec`, `bun --bun x`; `--prefix`, `-C`, `--cwd` move the directory) and a wrapper's options with their arguments (`nice -n 5`, `timeout -s KILL 5`, `sudo -u bob`, `env -u X`; `sudo -D`, `env -C` move the directory) are skipped. `eval`, `env -S` and each `find -exec` / `-execdir` / `-ok` command are parsed as `sh -c`. A command name expanded at run time (`N=npx; $N cowsay`) cannot be followed and is flagged. The create forms run an initializer package `create-<name>` (`@scope` → `@scope/create`, `@scope/x` → `@scope/create-x`): `npm create` / `npm init <x>` and `bun create <x>` are judged as `npm exec` / `bunx create-<x>` (a GitHub `user/repo` template is fetched), `pnpm create` and `yarn create` always fetch; `npm init` without an initializer is not flagged. `deno run|x|serve` on an `npm:`, `jsr:` or URL module is flagged. npm documents that without a TTY, or in CI, `--yes` is assumed: a missing name is downloaded and run without a prompt.
- Commands expressible as a permission prefix use a `permission` target instead of a `guard` target.
- The guard is a tripwire. The sandbox and permissions are the protection boundary.

### 9.3 Asks nobody can answer

In an autonomous run nobody answers an ask, and Claude Code denies it unseen (a PreToolUse `ask`, and a permission prompt without a `PermissionRequest` hook decision). Forge has an independent reviewer answer instead.

- **When.** `FORGE_ASK_REVIEW=always`; or, by default (`auto`), when `CLAUDE_CODE_ENTRYPOINT` is `sdk-cli`, `sdk-ts`, `sdk-py` or `claude-code-github-action` (`claude -p`, the Agent SDK, the GitHub Action; an interactive session is `cli`). Never with `FORGE_ASK_REVIEW=never`, in `dontAsk` mode (`permission_mode`), or inside a review (`FORGE_REVIEWER` set). Otherwise the scripts print nothing and Claude Code behaves as usual.
- **Guard asks.** `guard.sh` passes an `ask` to `review.sh pre <rule-id> <concern> <question>` (the concern: the rule's description and the check's finding; the question: the rule's `review`), and prints its `allow` or `deny` (`permissionDecisionReason`: `forge:<rule-id> reviewed by an independent session, nobody being there to answer: <reason>`). A PreToolUse `allow` does not override settings `deny` rules.
- **Permission prompts.** `review.sh` on PermissionRequest prints `{"decision":{"behavior":"allow"}}` or `{"behavior":"deny","message":"forge: denied by an independent review, nobody being there to answer: <reason>"}`. The project-level hook prints nothing when `~/.claude/skills/forge-user/scripts/review.sh` exists, so a request is reviewed once.
- **Test edits.** `test-edit.sh` passes its ask to `review.sh pre` the same way, with the user's request for the turn (§10.5).
- **The reviewer** is `claude -p` with a prompt holding only the repository path, the tool name, its `tool_input`, the user's request for the turn when reviewing a test edit, and, for a rule's ask, the rule, its concern and one question (the rule's `review`, else whether the action is, as far as that concern goes, normal work and safe unattended); not the transcript or the conversation. For a rule's ask it judges only that question: the rest of the action (other commands in it) is covered by other rules and is no reason to deny. It runs with `FORGE_REVIEWER=1`, `--tools Read,Grep,Glob --allowedTools Read,Grep,Glob --permission-mode dontAsk --setting-sources '' --settings '{"disableAllHooks":true}' --strict-mcp-config --no-session-persistence --output-format json --json-schema <decision allow|deny, reason>`, `--max-turns` 12 (`FORGE_REVIEW_MAX_TURNS`) and `FORGE_REVIEW_MODEL` as `--model` when set. Without a rule (a permission prompt) its instructions are: allow only what is clearly normal work on the repository and safe unattended; deny anything irreversible, publishing or pushing, touching secrets, installing unverified packages, weakening security, tests or tool configuration, or running code from the internet; when in doubt, deny.
- **Cache.** A verdict on a rule's ask is reused for the same rule, the same `tool_input` (whitespace runs collapsed), the same concern and the same request for the turn within a session (`review-cache` next to the log): an approval of a test edit given for one request is not reused once the user asks for something else.
- **No verdict** (claude missing, no `structured_output`, a decision other than allow/deny): nothing changes; the ask stays unanswered (denied). A timeout of the guard hook blocks the call (`onFailure: "block"`).
- **Log.** One tab-separated line per review (time, session, decision, tool, input, reason) in `<state>/reviews.log` inside a Forge project (§8), else `~/.claude/forge-reviews.log`.
- **Trust.** Claude Code loads a project's skills-dir plugin only in a trusted workspace; a headless run in an untrusted one runs no project hook. The user-level plugin is not affected.

## 10. `check`

### 10.1 `.forge/check.sh [--changed | --since <ref> | --all]`

One entry point for the Stop hook, the optional pre-commit hook and CI. Each mode checks a **change** against a **base**:

| Mode | Used by | Changed files | Base |
|---|---|---|---|
| `--turn <snapshot>` | Stop (§10.2) | files whose content differs from the turn snapshot | the tree at the start of the turn |
| `--changed` | pre-commit | `git status --porcelain --no-renames` (staged, unstaged, untracked) | `HEAD` |
| `--since <ref>` | CI on a pull request | `git diff --name-only <base>` plus untracked files | `git merge-base <ref> HEAD`; none → fail with `forge:forge/since` |
| `--all` | CI on the default branch | the whole project | `HEAD` (for the comparisons of steps 2–3) |

Paths are listed NUL-separated (`-z`, here, in the turn snapshot and in the base tree of §10.6), so a name with non-ASCII characters is taken as it is; git would otherwise print it quoted and octal-escaped, and the file would drop out of the change.

**Forge's own files.** Files under `.forge/` (check.sh, the helper scripts) are never part of a change or of `--all`'s scope, and a tool run whose findings are all in `.forge/` passes (`forge:<id> ok (its findings are only in Forge's own .forge/ files)`): integrity (step 1) covers them, the project's rules and tools do not.

**Added lines.** Outside `--all`, each changed file's added lines are computed with `git diff --no-index -U0` against its base copy; a file absent from the base counts fully. Formatting is whole-file, so a formatter run after an edit may rewrite lines the change never touched: when a check in scope has a `format_stdin`, the base copy is also formatted with it, and a line counts only if it is added against both the base copy and the formatted one; since the two diffs may align repeated lines differently (a duplicated line is the first copy in one, the second in the other), a line added against the base whose text is that of a line added against the formatted base, not matched by number, also counts (each such line once). When no filter applies, its tool is absent, or it fails or prints nothing on the base (e.g. the base does not parse), the plain comparison applies. An edited line counts as added, and so does moved code. ast-grep `forbid` matches and `lint` diagnostics count only when they overlap an added line (a match spanning an edited line counts). `no-increase` / `no-decrease` rules and whole-project checks (typecheck, migrations, tests) are not line-scoped.

Runs in order, stops at the first failing step, prints `forge:<rule-id>` lines:

1. **Integrity:** every owned file under `.forge/` and `.claude/` matches its hash in `.forge/manifest.sha256`, and no other file sits in Forge's directories. check.sh never parses `forge.lock.json`, so the lock's formatting does not matter.
2. **Applied migrations:** any file under `alembic/versions/` or `prisma/migrations/` that exists in the base and is modified or deleted → fail.
3. **Tests and counts:** test files present in the base and deleted → fail; ast-grep and grep `no-increase` / `no-decrease` rules compared with the base (`forge:<id> <path>: <before> -> <after>`).
4. **Forbid rules** (ast-grep and grep), on added lines outside `--all` (`forge:<id> <path>:<line>[:<column>] <description>`).
5. **Format, lint, typecheck:** `check` targets of stages `format`, `lint`, `typecheck`, using project binaries: `changed_run` on the changed files with `--turn` and `--changed`, `run` (read-only, whole project) with `--since` and `--all`. Outside `--all`, a failing run's error diagnostics are attributed: `lint` diagnostics count on added lines, `typecheck` diagnostics (and a whole-project `format` run's) on changed files. When none counts, the check prints `forge:<rule-id> ok for the added lines (…)` or `ok for the changed files (…)` and passes; otherwise it prints only the diagnostics that count. Output naming no file at all (configuration error, crash) fails as before. Diagnostic locations are read from the tools' plain output (`path(l,c)`, `path:l:c`, `--> path:l:c`; `Would reformat: path` and Biome's `path format` name a file only; absolute paths under the repository are made relative; warning-only lines are ignored). A failing run whose output has warnings and no error (ty with `terminal.error-on-warning` exits 1 on warnings alone) prints `forge:<rule-id> ok for the added lines (only warnings)` or `ok for the changed files (only warnings)` and passes; the edit-time hooks stay silent on it. `--all` fails on any error, and on the tool's exit status.
6. **Migration checks:** `check` targets of stage `migration`, only if a `when_changed` path changed and, when `requires_local_db` is set, the configured database host is `localhost`, `127.0.0.1`, `::1` or a Unix socket. Otherwise print `forge:<rule-id> skipped: no local database` and continue.
7. **Tests:** `check` targets of stage `test` (`changed_run` with `--changed` and `--turn`). Under `--turn`, failing tests are first compared with the start of the turn (§10.6).

**Failure output.** A check whose output is not attributed (tests, migrations, a crash) prints `forge:<rule-id> failed` and an excerpt of at most 15 lines (25 for tests; after a comparison with the start of the turn, 20 or 12, §10.6), the failure first: output that fits is shown as it is; otherwise its summary lines (Vitest `Test Files … failed` / `Tests … failed`, pytest's short summary `FAILED …` / `ERROR …` and `=== … failed … ===`), then the failure details (each Vitest `FAIL` line with the error lines of its block and its first stack frame outside `node_modules`; pytest's `____ test ____` headers, `E ` lines and `path:line: Error` locations), or, in output of no known format, the lines naming an error or a failure (not warnings); then `--- output:` and the rest of the output in order, warnings included, as far as the budget allows, and `... (<k> more lines)`.

A check with a `timeout` (seconds) that runs past it is stopped with its process tree and fails with `forge:<rule-id> failed: timed out after <n>s and was stopped …` first, then its output until then. A check.sh that receives SIGTERM, SIGINT or SIGHUP stops every process it started (each check with its process tree, the timeout watchers) and exits 143.

### 10.2 Stop hook (`stop.sh`)

`tree_hash` (in `lib.sh`) hashes `git diff HEAD` plus the untracked files (names and contents).

0. **Turn start** (`turn-start.sh`, UserPromptSubmit): write the snapshot `<state>/<session_id>.turn/` (§8): `head` (the commit), `tree` (`tree_hash`), `dirty` (`<path><TAB><blob hash>` per uncommitted path, `-` when deleted) and `files/<path>` (a copy of each dirty file up to 1 MiB, for added lines); reset the block counter `<state>/<session_id>.blocks`. Print nothing.
1. With a snapshot: if `tree_hash` and `HEAD` equal the snapshot's, the tree did not change during the turn: exit 0. The change is then what differs from the snapshot, commits made during the turn included; the user's own uncommitted work from before the turn is not part of it. Without a snapshot (no prompt seen yet, e.g. the first Stop after `forge apply`), the change is every uncommitted file (`--changed`).
2. If the tree (and, with a snapshot, `HEAD`) equals the last passing one (`<snapshot>/pass`, else `<state>/last-pass`), exit 0.
   Then, if it equals the last failing one and that failure was only tests that also fail at the start of the turn (`<…>/fail.unverified` is not empty, §10.6): the agent stopped again without changing anything after being told what to do. Record the tree as passing, reset the counter, and print `{"systemMessage":"forge: <n> test(s) could not be verified: they fail with and without Claude's change[, and their errors look environmental: <error>; …]. Not verified: <check> <test>; … Run them once the environment is set up."}` (at most 10 tests named): the stop is allowed and the user knows what was not verified.
3. If the block counter is ≥ 3: reset it, print `{"systemMessage":"forge: check failed 3 times. Handing over to you."}`, exit 0.
4. If the tree (and, with a snapshot, `HEAD`) equals the last failing one (`<snapshot>/fail`, else `<state>/last-fail`), the check is not run again: increment the counter and block with `forge: nothing changed in the working tree since this check failed, so it was not run again (it runs again once a file changes):` followed by the stored reason (`<…>/fail.reason`). The checks give the same result on the same tree, and a test suite can take minutes; the repeated block still counts toward step 3, so the agent gets the same chance to respond as before, at no cost.
5. Run `check.sh --turn <snapshot>` (or `--changed`), with `FORGE_UNVERIFIED=<…>/fail.unverified`. The whole run has one time budget under the hook's 600 s timeout (a hook Claude Code stops blocks nothing): `FORGE_STOP_BUDGET` seconds (540), passed as `FORGE_DEADLINE`; each check, and the re-run of §10.6, gets at most what is left of it, a check with nothing left is not started, and one stopped by it fails with `forge:<rule-id> failed: stopped after <n>s: the Stop hook's time budget (<b> s, FORGE_STOP_BUDGET) ran out before it finished; its output until then:` (a re-run stopped by it makes the comparison not possible).
6. Pass: record the tree as passing, forget the failing one, reset the counter, print nothing.
7. Fail: record the tree (as it is after the check, formatters included) and the reason as failing, increment the counter, print `{"decision":"block","reason":"<check output>"}`: the failing checks first, then advice (§10.7), then the `skipped`, `ok for` and `accepted` notes sorted by line (checks print them in run order, which differs when advice is deferred: the same tree gives the same reason), at most 40 lines and 6000 characters. `fail.unverified` is kept only when check.sh exited 3. When check.sh exited 4 (advice was the only failure), the tree is recorded as passing instead: every check passed and the advice is given once, so stopping again on that tree is allowed. A failing tree is recorded whatever advice came with it: step 4 replays the whole reason, and the advice is not given again once the tree changes.

### 10.3 Pre-commit and CI

- `forge apply` offers the commit-time check `sh .forge/check.sh --changed`:
  - without a hook manager, as `.git/hooks/pre-commit` (local to the clone), if that file does not exist (an existing hook is never touched): interactively, or with `--yes` or `--pre-commit`;
  - with a hook manager, interactively or with `--pre-commit` (not `--yes` alone), as a block between `# forge: begin (forge remove deletes this block)` and `# forge: end` in its config: `.pre-commit-config.yaml` gets a `repo: local` entry appended to `repos` (`id: forge-check`, `name: forge check`, `entry: sh .forge/check.sh --changed`, `language: system`, `pass_filenames: false`); `lefthook.yml` (`.yaml`, `.lefthook.yml`) a `forge-check` command under `pre-commit: commands:`; `.husky/pre-commit`, or `<core.hooksPath>/pre-commit` inside the repository, the line `sh .forge/check.sh --changed || exit 1` before the first top-level `exec`/`exit` (not one inside an `if`, `case`, loop, group or function), else at the end (the file is created when missing). A CRLF file keeps CRLF line ends. A YAML config is edited only when the result parses to the original plus Forge's entry; Forge leaves a config it cannot edit safely (an inline `repos: []`, lefthook `jobs`, a hooks path outside the repository) to the user. A config that already mentions `.forge/check.sh` is not edited.
- The block is recorded in the lock's `blocks` (§12): `apply` adds it back when it is missing, `status` reports it missing or modified, `remove` deletes it (and a file Forge created, when nothing else is left). In a config Forge created, the block holds the parent keys (`repos:`, `pre-commit:` / `commands:`); `remove` keeps them when the user has added entries under them. `remove` leaves the block to the user, and exits 1, when its markers are not one begin line followed by one end line, or when a YAML config without it would not parse to the original minus Forge's entry.
- `apply` and `status` say clearly when commits are not checked (`Pre-commit: NOT INSTALLED, so commits are not checked. To install: …`). A hook manager's config counts only when the manager is installed in this clone (git runs it): pre-commit and lefthook have written git's `hooks/pre-commit` (`pre-commit install`, `lefthook install`), Husky has set `core.hooksPath` to `.husky/_` or `.husky`; otherwise `Pre-commit: <file> runs the check (forge), but <manager> is not installed in this clone, so commits are not checked. To activate: <command>`. `status --json` has `pre_commit: {installed, file?, forge?, activate?}`.
- `forge status` prints the CI steps: `sh .forge/check.sh --since origin/<default branch>` for pull requests (the checkout needs the base branch's history: `fetch-depth: 0`), and `sh .forge/check.sh --all` for the default branch. The default branch is `origin/HEAD`, else `main`.

### 10.4 Config ratchet

`python/config-ratchet` and `typescript/config-ratchet` are `check` targets (stage `lint`) running the engine helper `forge_ratchet <runtime> <script>`. The script is `.forge/helpers/ratchet.py` (run with the project's own `.venv/bin/python`, 3.11+ for `tomllib`) or `.forge/helpers/ratchet.mjs` (run with `bun`), written only when the rule is active; a project formatter may rewrite it (§12). It compares each changed config file with its copy in the base (§10.1) and prints one line per loosened setting, `<path>: <setting> <before> -> <after>` (or `adds <x>`, `no longer has <x>`). Only the relevant sections are parsed, so dependency changes never trigger it:

| File | Loosened when |
|---|---|
| `pyproject.toml` `[tool.ruff]`, `ruff.toml`, `.ruff.toml` | a selected code is no longer covered by `select` / `extend-select` (prefixes and `ALL` count); a code added to `ignore` / `extend-ignore` or to a glob of `per-file-ignores`; a path added to `exclude` / `extend-exclude` |
| `[tool.ty]`, `ty.toml` | a `rules` level lowered (`error` > `warn` > `ignore`) or removed; `terminal.error-on-warning` no longer true; `src.exclude` entries added; an `overrides` entry added |
| `[tool.pytest.ini_options]` | `addopts` gains `--ignore`, `--ignore-glob`, `--deselect`, `-k`, `-m`, `--lf`, `--sw` or `--continue-on-collection-errors`; `testpaths` entries removed; `norecursedirs` entries added; `xfail_strict` no longer true |
| `[tool.coverage]` | `report.fail_under` lowered or removed; `omit`, `exclude_lines` or `exclude_also` entries added |
| `biome.json`, `biome.jsonc` | a rule's level lowered (`error` > `warn` > `info` > `off`) or removed; a group's level lowered; `recommended`, `linter.enabled` or `formatter.enabled` turned off; negated `files.includes` / `linter.includes` entries added; an `overrides` entry added or changed |
| `tsconfig*.json` | a strict flag (`strict`, `noImplicitAny`, `strictNullChecks`, … `noUncheckedIndexedAccess`, `exactOptionalPropertyTypes`, `noUnused*`, `checkJs`) no longer true; `skipLibCheck` or `ignoreDeprecations` added; `exclude` entries added; `include` entries removed; `extends` changed |
| `package.json` `scripts` | a script named like `test`, `check`, `lint`, `type…`, `e2e`, `ci`, `verify` removed, or given `--passWithNoTests`, `\|\| true`, `--no-errors-on-unmatched`, an exclude/ignore/skip flag, `--max-warnings N`, `--silent` |

A finding whose exact line appears in `forge.yaml` (an `accept` entry) is not reported. Otherwise the check fails with the findings and how to accept one. When the runtime is missing the check is `skipped: <tool> not installed`; when the script cannot run (no `tomllib`, a crash) it prints `forge:<id> skipped: …` and does not block. JS and TS config files (`vitest.config.*`, `playwright.config.*`) are code and are not parsed: they are behind permission `ask` only. The ratchet applies to everyone: a human loosening a config is stopped at pre-commit and in CI too, until the change is accepted in `forge.yaml`.

### 10.5 Test integrity

Claude may add tests, including in existing test files; changing a test that existed at the start of the turn (its setup and input data included), or removing it, needs approval. Sometimes the task requires it (it reverses the behaviour a test asserts), sometimes it only makes failing code pass; the user, or a reviewer given the user's request, tells them apart.

- **Tests.** With ast-grep (`--inline-rules`, the pinned version): pytest `test*` functions and methods with their decorators, keyed by enclosing classes and name; Vitest and Playwright `test(…)` / `it(…)` calls (any modifier: `.only`, `.skip`, `.each(…)`, …; not `test.describe`, `test.step` or hooks), keyed by the enclosing `describe` / `test.describe` names and the test name (`#2` for a repeated name). A test's **fingerprint** is its whole text (decorators, `.each(…)` table, setup, input data, control flow such as an early `return` or an `if False:` around asserts, and assertions) with whitespace, semicolons and trailing commas removed and `'` read as `"`, so reformatting is not a change. A test is **changed** when its fingerprint differs, **removed** when it is gone; a test whose key is new (an added test) is free.
- **When the edit is made** (`test-edit.sh`, PreToolUse `Edit|Write`, files in the scope of an active `no-test-edits` rule): the file at the start of the turn (the turn snapshot, else `HEAD`), now, and after the edit (Write: `content`; Edit: `old_string` replaced by `new_string`, once or with `replace_all`) are compared. When a test present at the start of the turn and now is changed or removed by the edit, the hook asks: `permissionDecisionReason` `forge:<rule-id> This edit changes what tests that existed before this turn assert, or removes them: <path> "<key>" (changed|removed), …. That is right when the task changes the behaviour they assert; otherwise it weakens the tests to make the code pass.` The tests are recorded in `<snapshot>/tests-pending/<tool_use_id>`. When nobody can answer (§9.3), `review.sh pre` answers with the rule's `review` question and the user's request for the turn (`<state>/<session_id>.prompt`, first 8000 bytes; not the conversation).
- **Approval.** On PostToolUse `Edit|Write` the call ran, so it was approved (by the user, the reviewer, or a permission mode): its pending tests move to `<snapshot>/tests-approved`. The turn's snapshot holds them, so they last one turn.
- **At Stop**, the engine helper `forge_test_edits` (`--turn` only) compares each in-scope file present in the base with the base and prints `<path>: test "<key>" was changed` or `… was removed` for each test changed or removed and not approved during the turn (a change made through Bash, say). The check then fails with: "These tests existed before this turn and were changed without approval. Restore them. If the task requires the change (it changes the behaviour they assert), make it with Edit or Write: Forge then asks for approval (the user, or a reviewer given the request)." Pre-commit and CI do not run it: a human edits tests on purpose. It complements `no-fewer-asserts`, `no-fewer-expects` and `no-new-skip`, which miss a changed expected value.
- **Test support code.** Top-level functions and classes not named `test*` / `Test*` in the rule's files (fakes, mocks, fixtures, helpers; pytest's scope includes `conftest.py`), and, in Vitest's, top-level declarations and `vi.mock` / `vi.doMock` calls (its scope adds `__mocks__/**`, `test-utils.*`, `test-helpers.*`, `vitest.setup.*`), keyed `support:<name>`. One present in the base and changed (text, whitespace aside) or removed prints `<path>: test support `<name>` was changed|removed` and fails the check with: "Test doubles, fixtures and helpers encode the interface the code's callers rely on. Keep that interface in production (e.g. a property alias for a moved attribute) unless the task removes it; if it does, carry on: this is reported once." Each one is reported once per turn (§10.7); the next Stop accepts it. Edits to it do not ask.
- **Limits.** An approval covers the test by key for the rest of the turn, so a later Bash change to the same test is not seen. A change outside the test (a module-level constant or a fixture it uses) is not a change to the test; changed fixtures and helpers are test support code, reported once. Renaming a test reads as a removal plus an addition, so it asks.

### 10.6 Failing tests at the start of the turn

Under `--turn` (Stop), a failing `test` check with a `rerun` form is compared with the start of the turn before it blocks, so the agent is not held responsible for tests that were already failing, typically because the environment lacks a service or the network (`ECONNREFUSED 127.0.0.1:5432`, `EAI_AGAIN`), and the missing environment is not silently accepted either.

1. **Failing tests** are read from the output: Vitest `FAIL  [|project| ]<file> > … > <test>` lines (a file that failed to load: `<file>`), pytest's short summary `FAILED <node id>` / `ERROR <node id>`. None found (a crash, a configuration error, a timeout): the check fails as usual.
2. **The base tree** is built in check.sh's temporary directory: the files git sees now (tracked and untracked, not ignored) copied with `tar`, this turn's changed files replaced by their copy at the start of the turn (files added during the turn left out). The repository, its index, its stash and its working tree are never written; no `git worktree` or `git stash` is used. Dependencies come from the current tree, the project's own code from the copy. Of the ignored paths (`git ls-files --others --ignored --exclude-standard --directory`; not `.forge/state/`):
   - a Python virtual environment (a directory with `pyvenv.cfg`) is a symbolic link to the real one. The repository paths its `.pth` files and editable-install finders (`__editable___*_finder.py` `MAPPING`) put on Python's path (uv, `pip install -e`, Poetry) are replaced by their copies, first on `PYTHONPATH` for the re-run; then the environment's `python`, run in the copy with that `PYTHONPATH`, checks that each project module it could import from the working tree (a package with `__init__.py`, a module) resolves to the copy;
   - each `node_modules` directory is rebuilt as a directory of links to its packages (`@scope/` entries one level down); a package that is a link to a directory of the repository outside `node_modules` (a pnpm, Yarn, npm or Bun workspace package, a `file:` dependency) is linked to that directory in the copy; Vite's and Vitest's caches (`.vite`, `.vitest`, `.cache`) are left out;
   - caches and reports a test run rewrites (`__pycache__`, `.pytest_cache`, `.mypy_cache`, `.ruff_cache`, `.hypothesis`, `.coverage`, `coverage`, `htmlcov`, `.nyc_output`, `.vite`, `.vitest`, `.turbo`, `.eslintcache`, `*.tsbuildinfo`) are left out;
   - everything else (`.env`, a SQLite file, `dist/`) is copied, so the re-run never writes the user's files; over `FORGE_BASE_COPY_KB` (262144) KiB in all, the copy is not made.

   When the copy cannot be made or isolated (the files to copy are too large, Python would import project code from the working tree, its check cannot run), the comparison is not possible (step 4).
3. **Only the failing test files** run there, with the check's `rerun` (`vitest run {files}`, `pytest -q {files}`; Vitest file paths are relative to their project, which a Vitest filter matches as part of the path), with the check's timeout. The tests failing there are compared by id with the tests failing now. A check that runs in workspace packages names its tests relative to the package; their file paths are made repository paths (`FAIL  packages/core/src/a.test.ts > …`, `FAILED pkg/tests/test_a.py::…`) as its diagnostics are, so the ids are compared, and the re-run selects them in that package of the base tree, the same way.
4. **A new failure** (fails now, not at the start of the turn): the check fails with `forge:<id> failed: <k> failing test(s) passed at the start of the turn:`, those tests (at most 8), `(<m> other failing test(s) also fail at the start of the turn: not caused by this change)` when there are some, then the output excerpt (§10.1) in at most 20 lines, so the reason still fits Stop's 40. check.sh exits 1. When the comparison is not possible (step 2, a re-run stopped at its timeout, or one that fails without naming a failing test), every failing test counts as new: `forge:<id> failed: <n> failing test(s); comparison with the start of the turn was not possible (<why>), so they count as new:`, the tests (at most 8) and the excerpt. A failure is never excused on a comparison that may have run the changed code.
5. **Only failures that were already there**: the check prints `forge:<id> <m> failing test(s) also fail at the start of the turn (re-run there), so this change did not cause them, but it is not verified by them either:`, the tests (at most 8), and, when the output has errors that look environmental, `Their errors look environmental:` with at most 5 of them as phrases (`Postgres refused the connection at 127.0.0.1:5432 (ECONNREFUSED)` — ports 5432, 3306, 6379, 27017, 1433, 9200, 5672, 11211 are named —, `DNS lookup for <host> failed (EAI_AGAIN): no network?`, else the line with `connection refused`, `ETIMEDOUT`, `getaddrinfo`, `timed out`, …), then what to do: set up what they need (`docker compose up -d with <file>` when a Compose file is at the root) and run them again, or, if that is not possible, stop again without changing files, which is then allowed and reported to the user (§10.2 step 2). Without environmental errors: check whether they relate to the task. Then the output excerpt (§10.1) in at most 12 lines. The tests and errors are appended to `$FORGE_UNVERIFIED` (`test<TAB><check id> <test id>`, `env<TAB><phrase>`), and the stage returns 3. check.sh exits 3 when that is the only failure.

The first such Stop always blocks: the agent learns that the environment, not its change, keeps the tests from verifying it, and gets one chance to fix that.

### 10.7 Advisory checks

Some checks report what Claude may have a good reason to leave as it is. They report each finding once per turn, through one engine helper:

- `forge_once <helper> [args]` runs `<helper>`; each line it prints is a finding. At Stop (`--turn`) a line not reported yet in this turn by this rule (`<snapshot>/reported`, `<rule-id><TAB><line>`) makes the check fail as **advice** with the whole output, and its lines are recorded; output with nothing new passes with `forge:<rule-id> accepted: reported once this turn`. A later finding of the same rule in the turn is therefore reported too, and what Claude already answered is not repeated; the next prompt's snapshot re-arms everything. In other modes (pre-commit, CI) any output fails the check (`typescript/lockfile-updated`).
- `forge_advise <helper> [args]` is `forge_once` at Stop only: pre-commit and CI do not run **advisory** checks. Claude is expected to act on the advice or to say why not.
- Test support code changed or removed (§10.5) goes through the same record (`forge_once_new`); alone, it is advice too.

Advice does not stop the steps after it (§10.1): the later stages still run, and its output is printed after every failing check (a failing test comes first even though advice comes from an earlier stage); check.sh exits 4 when advice was the only failure, and Stop then records the tree as passing (§10.2 step 7). A block that carries advice counts toward the hand-over after 3 blocks (§10.2 step 3) like any other: each block is a round trip for Claude, and new findings could otherwise keep a turn going.

| Helper | Prints |
|---|---|
| `forge_untested_change` | when the change has a code line (not blank, not only a comment) added or removed in an in-scope file and no existing file matching `FORGE_TEST_PATH_ERE` (pytest, Vitest and Playwright test paths) changed: "You changed behaviour in <files, up to 3> without adding or updating a test; add a test that fails without your change, or say why none is needed." |
| `forge_stub_sync` | for each changed `*.py` module that gained public names (the entries of `__all__` when it has one, else top-level `def` / `class` names without `_`; compared with the base's names) and each `.pyi` in the repository with the same module path inside its top-level package (packages are directories with `__init__.py` or `__init__.pyi`: `logfire/experimental/query_client.py` ↔ `logfire-api/logfire_api/experimental/query_client.pyi`), the new names the stub does not mention: "The stub <pyi> does not have <names>, new in <py>; add it there too." |
| `forge_docs_mentions` | when no documentation file changed (Markdown, MDX or reStructuredText under a `docs/` or `doc/` directory, and `README*`): the public names whose definition the change added, removed or edited on a removed or added line of an in-scope file (Python `def`, `class`, annotated names and parameters, module-level assignments; TypeScript and JavaScript `export` declarations, optional properties `name?:`, typed members `name: T;`, methods with a modifier; a line that only moved does not count; names starting with `_`, shorter than 4 characters or on a stop list of common words are dropped), then the documentation lines mentioning one (as a whole word; a name without `_` or an inner capital only in a code span or a fenced block): "These docs mention identifiers you changed; check they still describe the behaviour:" and up to 5 `path:line name` lines. |

## 11. Audit of tool targets

For each `tool` target of an active rule:

1. The tool is not present in the project → the rule's `ast-grep` target is used if it has one; otherwise `forge status` reports "not enforced here: <tool> not configured".
2. The tool is present → Forge writes each example to a temporary file inside the rule's scope and runs the project's own tool with the project's config, reading machine-readable output: `biome lint --reporter=json <file>…`, `ruff check --output-format json --stdin-filename`, `ty check --output-format gitlab <file>` (Forge's pinned ty through uvx, with the project's config: present wherever there is a `pyproject.toml` or `ty.toml`; `major` is an error, `minor` a warning), `tsc --noEmit -p <temporary tsconfig extending the project's>`. Biome and ty report by file, so the examples of a target are checked in one run (one per path when two examples share a path); each example gets the verdict a run on it alone would give, and runs alone when the shared run cannot tell (output not read, diagnostics left out, or a warning-only diagnostic while another file got an error).
3. Every `match` example gets the expected diagnostic **at a level that fails the tool's run**, and no `no_match` example gets it → satisfied (stricter configs satisfy automatically). A diagnostic fails the run when its severity is `error`, or when it is a warning and the run on that example exits non-zero without any error diagnostic (the tool fails on warnings, e.g. ty's `terminal.error-on-warning`). ruff has no warning level; tsc reports errors only.
4. Otherwise → `forge status` reports the rule as unsatisfied and prints its `snippet`; when the diagnostic is reported only as a warning, the detail says so. Forge never edits the config. Output that cannot be read → the audit is skipped with the tool's first line of output, never satisfied.

Audits run during `forge apply` and `forge status`, never in hooks.

## 12. Lock

`.forge/forge.lock.json` and `~/.claude/forge.lock.json`:

```json
{"forge_version":"0.5.0","claude_code_min":"2.1.295",
 "rules":{"<rule-id>":"<sha256 of rule file>"},
 "files":{"<path>":"<sha256>"},
 "settings":[{"file":".claude/settings.json","path":"permissions.ask","value":"Edit(.forge/**)"}]}
```

- `diff` and `status` report owned files whose hash differs, appended entries that are missing, and rules whose source changed since the last apply.
- `remove` deletes listed files and removes exactly the listed settings entries.
- **Validation.** Every command reading the lock validates it against its schema and refuses it as a whole (exit 1, nothing written or deleted) when a path is not one Forge writes: `files` keys must be owned paths (§4) or git's `hooks/pre-commit` (`git rev-parse --git-path`), `settings[].file` a settings file of that level, `blocks[].file` the file `hookManager` names for its `kind` (`.pre-commit-config.yaml`, `.husky/pre-commit`, `lefthook.yml` / `.yaml` / `.lefthook.yml`, `<core.hooksPath>/pre-commit`); all relative, without `.` or `..` segments, and inside the repository (or `~/.claude`) once symbolic links are resolved.
- **`blocks`** (present only when there is one): `[{file, kind, created, hash}]`, Forge's marked block in a hook manager's file (§10.3); `hash` is the sha256 of the block's lines, indentation removed.
- **Owned-file hashes** are sha256 of the content, except for `.json` files, which are hashed with spaces, tabs, CR and LF removed, and the scripts in `.forge/helpers/`, hashed also without `'`, `"`, `` ` ``, `,`, `;`, `(` and `)`: a formatter reflowing Forge's JSON, or rewriting its helper scripts (`ruff format .`, `biome check --write .`: layout, quote style, trailing commas, semicolons, parentheses), is not drift; any other change is. (A change made only of those characters, such as moving a parenthesis, is not detected: the price of letting formatters rewrite the scripts.) A file whose owned hash matches is not rewritten by `apply`. The lock is compared as JSON (formatting ignored).
- **`.forge/manifest.sha256`** (project level) repeats the owned-file hashes of `.forge/` and `.claude/` as `<sha256>  <path>` lines (sha256sum format) for `check.sh`. Non-JSON lines verify with `sha256sum -c`; JSON and helper lines need the normalised content, so `check.sh` hashes each file itself (`sha256sum`, `shasum -a 256` or `openssl`). The manifest and the lock do not hash themselves; `remove` deletes both.
- **Formatting.** During `apply`, the JSON files Forge generates (plugin manifest, `hooks.json`, the lock, and a `.claude/settings.json` that Forge creates) are formatted with the project's own Biome when installed (`biome format --stdin-file-path`), so the project's `biome ci .` passes. An existing settings file keeps its own indentation for appended entries. Project-owned files are never formatted.

## 13. Packs and rules

Rule ids are `<pack-id>/<name>`. Each line is one rule file. Headings are pack ids, so their nesting is the pack tree.

### safety (user)

| Rule | Targets |
|---|---|
| `safety/secret-files` | `permission` deny: `Read(**/.env)`, `Read(**/.env.local)`, `Read(**/.env.development)`, `Read(**/.env.production)`, `Read(**/.env.test)`, same with `Edit(...)`, `Read(**/*.pem)`, `Read(**/*.key)`, `Read(~/.ssh/**)`, `Read(~/.netrc)`, `Read(~/.npmrc)`, `Read(~/.pypirc)`, `Read(~/.pgpass)`, `Read(~/.git-credentials)`, `Read(~/.config/gh/hosts.yml)`, `Read(~/.docker/config.json)` |
| `safety/sandbox` | `sandbox` `enabled: true`; `network.allowedDomains`: `registry.npmjs.org`, `pypi.org`, `files.pythonhosted.org`, `github.com` |
| `safety/git-reset-hard` | `permission` deny: `Bash(git reset --hard:*)` |
| `safety/git-clean` | `permission` deny: `Bash(git clean:*)` |
| `safety/git-discard` | `guard` deny: `git checkout -- …`, `git checkout .`, `git restore` without `--staged` |
| `safety/git-force-push` | `guard` deny: `git push` with `--force`, `-f` or `+<ref>`; not `--force-with-lease` |
| `safety/git-force-with-lease` | `guard` ask |
| `safety/git-no-verify` | `guard` deny: `git commit` with `--no-verify` or `-n` |
| `safety/git-history-rewrite` | `permission` deny: `Bash(git filter-branch:*)`, `Bash(git filter-repo:*)`, `Bash(git reflog expire:*)`, `Bash(git stash clear:*)` |
| `safety/rm-root` | `guard` deny: recursive `rm` on `/`, `~`, `$HOME`, `..` |
| `safety/remote-script` | `guard` deny: `curl` / `wget` piped to `sh`, `bash`, `zsh`, `python`, `bun`, `node`; `base64 -d` piped to a shell |
| `safety/publish` | `permission` deny: `Bash(npm publish:*)`, `Bash(bun publish:*)`, `Bash(uv publish:*)` |
| `safety/secret-read-bash` | `guard` deny: `cat`, `less`, `head`, `tail`, `cp`, `source`, `grep`, `base64` on `.env` files (not `.env.example`), `~/.ssh/`, `~/.pgpass`, `~/.netrc` |
| `safety/dropdb` | `permission` deny: `Bash(dropdb:*)` |
| `safety/sql-destructive` | `guard` deny: `psql` with `DROP TABLE`, `DROP DATABASE`, `DROP SCHEMA`, `TRUNCATE` |
| `safety/docker-volumes` | `guard` deny: `docker compose down` / `docker-compose down` with `-v` or `--volumes`, `docker volume rm`, `docker volume prune`, `docker system prune --volumes` |
| `safety/package-add` | `permission` ask: `Bash(bun add:*)`, `Bash(uv add:*)` (hallucinated package names, slopsquatting) |
| `safety/lockfile-hand-edit` | `permission` ask: `Edit(**/pnpm-lock.yaml)`, `Edit(**/package-lock.json)`, `Edit(**/npm-shrinkwrap.json)`, `Edit(**/yarn.lock)`, `Edit(**/bun.lock)`, `Edit(**/uv.lock)`, `Edit(**/poetry.lock)`, `Edit(**/Cargo.lock)` (the package manager writes lock files) |
| `safety/lockfile-bash-write` | `guard` ask (`review`: is it a hand rewrite of a lock file the request asks for): the same lock files rewritten through Bash: `sed -i` / `perl -i` (`-0pi` included), a `>` / `>>` redirect or `tee` into one, `cp` / `mv` / `install` / `rsync` with one as the last argument, `dd of=`, `truncate`, a `python` / `node` / `bun` / `deno` / `ruby` command naming one and writing (`.write(`, `write_text`, `writeFile`, `Bun.write`, an `open(…, 'w')` mode) |
| `safety/prisma-ai-consent` | `guard` deny: any command containing `PRISMA_USER_CONSENT_FOR_DANGEROUS_AI_ACTION` (only the user gives that consent) |
| `safety/terraform-destroy` | `guard` ask: `terraform` / `tofu` `destroy`, `apply … -auto-approve` |
| `safety/git-stash-drop` | `guard` ask: `git stash drop` (`git stash clear` is denied by `git-history-rewrite`) |
| `safety/git-branch-force-delete` | `guard` ask: `git branch -D`, `--delete --force`, `-f -d` |
| `safety/ephemeral-exec` | `guard` ask, check `ephemeral-exec` (§9.2): packages fetched from a registry on the fly. `npx tsc`, `bunx prisma`, `uvx ruff` with the tool installed in the project are not asked; `npx some-typo` is |
| `safety/docker-privileged` | `guard` deny: `docker run` / `create` / `exec` with `--privileged`, `--pid=host`, `--ipc=host`, `--userns=host`, `--cap-add ALL` or `SYS_ADMIN`, `--security-opt seccomp=unconfined` or `apparmor=unconfined`, `docker.sock`; `run` / `create` mounting `/`, `/etc`, `/root`, `~` or `$HOME` (`-v`, `--volume`, `--mount source=`) |
| `safety/docker-push` | `guard` ask: `docker push`, `docker image push`, `docker compose push`, `docker buildx … --push` |
| `safety/forge-state` | `guard` deny: a command naming Forge's hook state (`forge-state`, `.forge/state`, `forge-review-cache`, `forge-reviews.log`, as whole names; §8) |
| `safety/forge-state-edit` | `permission` deny: `Edit(~/.claude/forge-state/**)` |

### python

| Rule | Targets |
|---|---|
| `python/format` | `check` format: `run: ruff format --check .`, `changed_run: ruff format {files}`, `format_stdin: ruff format --stdin-filename {file} -` |
| `python/lint` | `check` lint: `run: ruff check .`, `changed_run: ruff check {files}`, `fix_run: ruff check --fix --fixable I,UP --exit-zero --show-fixes {files}` (sorted imports and pyupgrade, when the project selects them; never F401 / F841) |
| `python/typecheck` | `check` typecheck: `run: ty check --output-format concise` (Forge's pinned ty, §15.2) |
| `python/no-explicit-any` | `tool` ruff `ANN401` (ty has no equivalent) |
| `python/no-bare-except` | `tool` ruff `E722` |
| `python/no-unscoped-ignore` | `tool` ty `blanket-ignore-comment`; `tool` ruff `PGH003` |
| `python/no-unknown` | `tool` ruff `ANN001`, `ANN002`, `ANN003`, `ANN201`, `ANN202`: parameters and return types annotated, so no receiver is Unknown (§15.1); an unannotated local's type is inferred (ty has no equivalent of `reportUnknownVariableType`) |
| `python/no-useless-cast` | `tool` ty `redundant-cast`, `disjoint-cast` |
| `python/no-deprecated` | `tool` ty `deprecated` (PEP 702 and typeshed deprecations) |
| `python/no-blocking-in-async` | `tool` ruff `ASYNC210`, `ASYNC230`, `ASYNC251` |
| `python/no-tls-bypass` | `tool` ruff `S501` |
| `python/no-shell-true` | `tool` ruff `S602` |
| `python/no-stale-ignore` | `tool` ty `unused-ignore-comment`, `unused-type-ignore-comment` |
| `python/aware-datetime` | `tool` ruff `DTZ005` |
| `python/no-new-suppressions` | `grep` no-increase: `# noqa`, `# ty: ignore`, `# type: ignore`, `# pyright: ignore` (suppression budget) |
| `python/config-files` | `permission` ask: `Edit(**/ruff.toml)`, `Edit(**/.ruff.toml)`, `Edit(**/ty.toml)` |
| `python/config-ratchet` | `check` lint: `forge_ratchet python ratchet.py` (§10.4), `when_changed`: the Python config files |
| `python/no-silent-except` | `tool` ruff `S110`, `S112` |
| `python/no-blind-except` | `tool` ruff `BLE001` |
| `python/no-blanket-noqa` | `tool` ruff `PGH004` |
| `python/no-unused-noqa` | `tool` ruff `RUF100` |
| `python/no-commented-out-code` | `tool` ruff `ERA001` |
| `python/no-print` | `tool` ruff `T201` (snippet: per-file-ignores for `scripts/**` and CLIs) |
| `python/no-dangling-task` | `tool` ruff `RUF006` |
| `python/log-exception` | `tool` ruff `TRY400` |
| `python/no-hardcoded-password` | `tool` ruff `S105`, `S106` (snippet: off under `tests/**`) |
| `python/no-sql-format` | `tool` ruff `S608` |
| `python/http-timeout` | `tool` ruff `S113` (requests without `timeout`; httpx with `timeout=None`) |
| `python/no-weak-hash` | `tool` ruff `S324` |
| `python/no-pickle` | `tool` ruff `S301` |
| `python/no-unsafe-yaml` | `tool` ruff `S506` |
| `python/jinja-autoescape` | `tool` ruff `S701` |
| `python/no-stub` | `ast-grep` forbid, not in `*.pyi` or migrations: a function whose body is only `pass` / `...` (docstrings and comments aside), or `raise NotImplementedError`, outside `@abstractmethod`, `@overload` and `Protocol` / `ABC` classes |
| `python/stub-sync` | `check` lint, advisory (§10.7): `forge_stub_sync` on `*.py` |
| `python/no-placeholder-comment` | `grep` forbid (case-insensitive): comments such as `TODO: implement`, "in a real implementation", "placeholder logic", "stub for now" |
| `python/env-example` | `check` lint: `forge_env_example py`, not with `--all`: a field added to a pydantic-settings `BaseSettings` class (`env_prefix`, `alias`, `case_sensitive` applied) appears in the nearest directory's `.env.example`, `.env.template`, `.env.sample`, `.env.dist` or `.env.<name>.example` (a `# KEY=` line counts): always without a default, also with one when the template lists at least half of the class's variables; a template listing none of them is not about that class |

context.md: uv and `.venv/bin`, ty, annotated parameters and returns; no blocking calls in `async def`; ignore comments name their rule; search for an existing helper (warn-once, logging, utils) before writing one.

### typescript

| Rule | Targets |
|---|---|
| `typescript/format-lint` | `check` lint: `run: biome ci .`, `changed_run: biome check --write {files}`, `format_stdin: biome format --stdin-file-path={file}`; no `fix_run`: `changed_run` applies Biome's safe fixes itself (removing unused imports or variables is unsafe, so it is not done) |
| `typescript/typecheck` | `check` typecheck: `run: tsc --noEmit -p tsconfig.json --pretty false --incremental --tsBuildInfoFile "$(forge_state_file tsc.tsbuildinfo)"` (plain diagnostics; incremental, TypeScript ≥ 4.0, with the build info under the repository root's `.forge/state/`, as an absolute path, one file per workspace package (`<package dir, / as ->.tsc.tsbuildinfo`), so a project's own or composite build info is never written and a package never gets a `.forge/` of its own) |
| `typescript/no-implicit-any` | `tool` tsc: expects `TS7006` on an untyped parameter (`noImplicitAny`, part of `strict`) |
| `typescript/strict-null-checks` | `tool` tsc: expects `TS18047` on a possibly-null access (`strictNullChecks`) |
| `typescript/strict-function-types` | `tool` tsc: expects `TS2322` on a parameter-bivariant assignment (`strictFunctionTypes`) |
| `typescript/no-explicit-any` | as §6.3 |
| `typescript/no-ts-ignore` | `tool` biome `lint/suspicious/noTsIgnore`; `ast-grep` forbid comment `// @ts-ignore` or `/* @ts-ignore */` |
| `typescript/no-non-null-assertion` | `tool` biome `lint/style/noNonNullAssertion`; `ast-grep` forbid `non_null_expression` |
| `typescript/no-double-assertion` | `ast-grep` forbid: `$X as unknown as $T`, `$X as any as $T` |
| `typescript/no-ts-nocheck` | `ast-grep` forbid comment `@ts-nocheck` |
| `typescript/ts-expect-error-reason` | `ast-grep` forbid comment `@ts-expect-error` with nothing after it |
| `typescript/no-ignore-deprecations` | `check` lint: no `"ignoreDeprecations"` in tracked `tsconfig*.json`, `when_changed: [tsconfig*.json]` |
| `typescript/no-focused-tests` | `tool` biome `lint/suspicious/noFocusedTests`; `ast-grep` forbid `it.only(…)`, `test.only(…)`, `describe.only(…)`, `test.describe.only(…)` in test files and `e2e/**` |
| `typescript/no-eval` | `tool` biome `lint/security/noGlobalEval`; `ast-grep` forbid `eval(…)`, `globalThis.eval(…)`, `window.eval(…)` |
| `typescript/unchecked-index` | `tool` tsc: expects `TS18048` on an unchecked array element (`noUncheckedIndexedAccess`; snippet adds it to `strict`) |
| `typescript/no-new-suppressions` | `grep` no-increase: `biome-ignore` (every form), `@ts-expect-error` (suppression budget; `@ts-ignore` and `@ts-nocheck` are forbidden outright) |
| `typescript/no-empty-block` | `tool` biome `lint/suspicious/noEmptyBlockStatements` |
| `typescript/no-swallowed-catch` | `ast-grep` forbid: a `catch` that neither throws nor uses its error; `.catch(() => {})`, `.catch(() => undefined)`, `.catch(() => null)` |
| `typescript/no-console` | `tool` biome `lint/suspicious/noConsole` (snippet allows `warn` and `error`) |
| `typescript/no-unused` | `tool` biome `lint/correctness/noUnusedImports`, `lint/correctness/noUnusedVariables` |
| `typescript/no-secrets` | `tool` biome `lint/security/noSecrets` (snippet: error, `entropyThreshold` 50, off for tests and generated files) |
| `typescript/no-unused-suppressions` | `check` lint: `biome lint` output has no `suppressions/unused` diagnostic (Biome reports it only as a warning, and no setting raises it) |
| `typescript/no-stub` | `ast-grep` forbid, not in `*.d.ts`: a `throw` whose message starts with "Not implemented", "unimplemented" or "TODO"; a named function or method (not a constructor) whose body is empty or only comments |
| `typescript/no-placeholder-comment` | `grep` forbid (case-insensitive): the Python list, after `//`, `/*` or `*` |
| `typescript/config-files` | `permission` ask: `Edit(**/biome.json)`, `Edit(**/biome.jsonc)`, `Edit(**/tsconfig*.json)` |
| `typescript/config-ratchet` | `check` lint: `forge_ratchet bun ratchet.mjs` (§10.4), `when_changed`: Biome, tsconfig and `package.json` files |
| `typescript/lockfile-updated` | `check` lint: `forge_lockfile_updated`: a `package.json` whose `dependencies`, `devDependencies` or `optionalDependencies` changed since the base needs its lock file (`pnpm-lock.yaml`, `package-lock.json`, `npm-shrinkwrap.json`, `yarn.lock`, `bun.lock`; next to it, else at the root) to have a changed line naming each changed dependency; `bun.lock` is skipped when `typescript/bun/lock-sync` is active (`forge_check_active`); blocks once per turn (`forge_once`) |
| `typescript/env-example` | `check` lint: `forge_env_example ts`, as `python/env-example` for the UPPER_CASE keys of a Zod env schema (t3-env `createEnv`, or `z.object(…)` with `process.env`) whose value is a `z.` chain; a key with `.default()`, `.optional()` or `.catch()` counts as having a default |

context.md: strict TypeScript and Bun; TypeScript 6 deprecations are migrated, never silenced with `ignoreDeprecations`; no `!` or `as unknown as`; search for an existing helper (warn-once, logging, `util.*`) before writing one.

### python/uv

| Rule | Targets |
|---|---|
| `python/uv/lock-sync` | `check` lint: `run: uv lock --check`, `when_changed: [pyproject.toml]` |
| `python/uv/no-pip` | `guard` ask: `pip install`, `pip3 install`, `uv pip install` |
| `python/uv/exclude-newer` | `check` lint: `forge_uv_cooldown`: `[tool.uv]` (or `uv.toml`) `exclude-newer` is a relative duration of at least 3 days (`"7 days"`, `P1W`) and `required-version` is `>=0.9.17`; `when_changed`: `pyproject.toml`, `uv.toml`, `uv.lock` |

### python/fastapi

| Rule | Targets |
|---|---|
| `python/fastapi/no-on-event` | `ast-grep` forbid: `@app.on_event($$$)` |
| `python/fastapi/annotated-depends` | `tool` ruff `FAST002` |
| `python/fastapi/cors-credentials-wildcard` | `ast-grep` forbid: `add_middleware(CORSMiddleware, …)` with both `allow_origins=["*"]` and `allow_credentials=True` |

context.md: "FastAPI. Dependencies use `Annotated[T, Depends(...)]`. Startup and shutdown use `lifespan`. Request and response bodies are Pydantic v2 models."

### python/sqlalchemy

| Rule | Targets |
|---|---|
| `python/sqlalchemy/no-query-api` | `ast-grep` forbid: `$S.query($$$)` |
| `python/sqlalchemy/no-declarative-base` | `ast-grep` forbid: `declarative_base($$$)` |
| `python/sqlalchemy/no-backref` | `ast-grep` forbid: `relationship($$$, backref=$_, $$$)` |
| `python/sqlalchemy/mapped-column` | `ast-grep` forbid: `$N = Column($$$)` inside a class body |
| `python/sqlalchemy/no-raw-execute` | `ast-grep` forbid: `$X.execute("$$$")` with a string literal |
| `python/sqlalchemy/async-expire-on-commit` | `ast-grep` forbid: `async_sessionmaker(…)` without `expire_on_commit=False` |
| `python/sqlalchemy/no-text-fstring` | `ast-grep` forbid: `text()` whose SQL is an f-string, `.format()`, `%` or `+` concatenation |
| `python/sqlalchemy/fk-index` | `check` lint, advisory (§10.7): `forge_advise forge_helper python fk_index.py` (§13.4), skipped for a MySQL / MariaDB `forge_db_url` |

context.md: SQLAlchemy 2.0 async models, queries and sessions; no lazy loading in async sessions (`selectinload()`, `lazy="raise"`); values reach `text()` as bound parameters, with f-strings, `.format()`, `%`, `+` in `text()` and literal strings in `.execute()` rejected, and statements that cannot take parameters (`SET`, `SET LOCAL`) free to embed a validated value.

### python/sqlalchemy/alembic

| Rule | Targets |
|---|---|
| `python/sqlalchemy/alembic/check` | `check` migration: `run: alembic check`, `when_changed: [**/models/**, **/models.py, alembic/versions/**]`, `requires_local_db: true` |
| `python/sqlalchemy/alembic/single-head` | `check` migration: `run: test "$(alembic heads | wc -l)" -le 1`, `when_changed: [alembic/versions/**]` |
| `python/sqlalchemy/alembic/no-downgrade-base` | `guard` deny: `alembic downgrade base` |
| `python/sqlalchemy/alembic/downgrade` | `guard` ask: `alembic downgrade`, `alembic stamp` |
| `python/sqlalchemy/alembic/migration-lint` | `check` migration: each changed revision's upgrade rendered offline (`alembic upgrade <down_revision>:<revision> --sql`) and linted with Squawk (§13.1); `--all`: the head revision; merge revisions are skipped; `when_changed: [alembic/versions/**]` |
| `python/sqlalchemy/alembic/model-registered` | `check` lint on `**/models/**`: `forge_models_registered`: a module of a models package with a mapped class (`__tablename__`) is named in the package's `__init__.py` when that file imports other table modules of the package (the registry), or in `alembic/env.py`; otherwise `Base.metadata` lacks the table and `alembic check` passes without its migration |
| `python/sqlalchemy/alembic/fk-index` | `check` lint on `alembic/versions/**`, advisory (§10.7): `forge_advise forge_alembic_fk_index`: in `upgrade()`, each column given a foreign key (`sa.Column(…, sa.ForeignKey(…))`, `sa.ForeignKeyConstraint([…])`, `op.create_foreign_key(…, […], …)`) is the first column of an `op.create_index`, `sa.Index`, primary key or unique constraint or `CREATE INDEX`, or has `index=True`, `unique=True` or `primary_key=True`; skipped for a MySQL / MariaDB database URL |
| `python/sqlalchemy/alembic/sqlite-batch` | `check` lint on `alembic/versions/**`: `forge_alembic_batch`: when `alembic/env.py` sets `render_as_batch=True` or `alembic.ini` has a sqlite URL, no `op.alter_column`, `op.drop_column`, `op.create_foreign_key`, `op.drop_constraint`, `op.create_unique_constraint`, `op.create_check_constraint`, `op.create_primary_key` outside `op.batch_alter_table()`, except under an `if` on the dialect |
| `python/sqlalchemy/alembic/data-migration-downgrade` | `check` lint on `alembic/versions/**`, advisory (§10.7): `forge_advise forge_alembic_data_downgrade`: an `upgrade()` that changes rows (`UPDATE … SET`, `INSERT INTO`, `DELETE FROM` with `execute(`, `sa.update/insert/delete(…)`, `.update()/.insert()/.delete()`, `op.bulk_insert(`, `Session(`) has a `downgrade()` that changes rows the same ways or has a `raise` statement; reported at the `downgrade` line |

The migration `check` lint rules above examine the changed revisions, and with `--all` only the head revisions. A required column added without a default (`op.add_column(…, nullable=False)` without `server_default`) is already caught by `migration-lint` (Squawk `adding-required-field`, §13.1).

context.md: "Alembic with an async engine: `alembic/env.py` runs migrations through `run_async_migrations()` and `connection.run_sync()`. New revisions: `alembic revision --autogenerate -m \"<change>\"`. Committed revisions are never edited."

### python/pydantic

| Rule | Targets |
|---|---|
| `python/pydantic/v2-api` | `ast-grep` forbid: `$M.dict()`, `$M.json()`, `$M.parse_obj($$$)`, `$M.parse_raw($$$)`, `@validator($$$)`, `@root_validator($$$)`, `class Config:` inside a `BaseModel` subclass |

context.md: "Pydantic v2: `model_validate`, `model_dump`, `model_config = ConfigDict(...)`, `@field_validator`, `@model_validator`, `Field(pattern=...)`, `TypeAdapter(T).validate_python(...)` (not `parse_obj_as`)."

### python/pytest

| Rule | Targets |
|---|---|
| `python/pytest/run` | `check` test: `run: pytest -q`, `changed_run`: changed test files plus `tests/**/test_<module>.py` for each changed module and, at Stop, the test files edited during the turn (`<snapshot>/touched`, §10.5) even when the edit was reverted; `pytest --testmon` when the project depends on pytest-testmon; timeout 300 s |
| `python/pytest/no-new-skip` | `ast-grep` no-increase on test files: `@pytest.mark.skip($$$)`, `@pytest.mark.skipif($$$)`, `@pytest.mark.xfail($$$)`, `pytest.skip($$$)`, `@unittest.skip($$$)`, `pytest.importorskip($$$)` |
| `python/pytest/no-fewer-asserts` | `ast-grep` no-decrease on test files: `assert $$$` |
| `python/pytest/no-test-env-branch` | `ast-grep` forbid on non-test files: `PYTEST_CURRENT_TEST` lookups, `"pytest" in sys.modules` |
| `python/pytest/no-blind-raises` | `tool` ruff `B017` |
| `python/pytest/no-tautological-assert` | `ast-grep` forbid on test files: `assert` on a literal (`True`, a number, a string), `assert $X == $X`, `assert $X is $X` |
| `python/pytest/no-mock-increase` | `ast-grep` no-increase on test files: calls to `patch`, `patch.object`, `patch.dict`, `patch.multiple`, `Mock`, `MagicMock`, `AsyncMock`, `NonCallableMock`, `create_autospec` (bare, `mock.`, `unittest.mock.`, or through an alias: `from unittest.mock import patch as p`, `import unittest.mock as um`, `from unittest import mock as m`), and `mocker.patch*`, `mocker.spy`, `mocker.stub`, `mocker.Mock*`; `monkeypatch` is not counted (pytest's way to set configuration) |
| `python/pytest/no-test-edits` | `check` lint: `forge_test_edits` on `**/test_*.py`, `**/*_test.py`, `**/conftest.py`; edits ask (§10.5); `review`: is the change required by the user's request, or does it weaken the tests |
| `python/pytest/untested-change` | `check` lint, advisory (§10.7): `forge_untested_change` on non-test `*.py` (not migrations) |

context.md: tests assert behaviour, never weakened; "When the task changes behaviour an existing test asserts, update that test in place with Edit or Write (keep its name; Forge asks for approval) and say so in your summary. Otherwise never edit an existing test to make the code pass."

### python/django

Detected by `django` (pypi) or a `manage.py` at the root or one level down.

| Rule | Targets |
|---|---|
| `python/django/makemigrations` | `check` migration: the shallowest tracked `manage.py`, `python <manage.py> makemigrations --check --dry-run --no-input` (no database needed: Django only warns when it cannot check the migration history); fails on "Migrations for", any other failure (settings that need an environment variable, a missing module) prints `skipped:` with the error's last line; `when_changed`: models, migrations, settings; timeout 120 s |
| `python/django/no-raw-sql-format` | `tool` ruff `S610` (`.extra()` with a non-literal `select` / `where` / `tables` / …), `S611` (`RawSQL()` with non-literal SQL); SQL strings built with an f-string, `.format()`, `%` or `+` (also through a variable) are `python/no-sql-format` (`S608`) |
| `python/django/flake8-django` | `tool` ruff `DJ001`, `DJ003`, `DJ006`, `DJ007`, `DJ013` |

context.md: migrations generated and committed with model changes; SQL parameters; `select_related()` / `prefetch_related()` against N+1; `transaction.atomic()` and `transaction.on_commit()`.

### python/django/drf

Detected by `djangorestframework` (pypi).

| Rule | Targets |
|---|---|
| `python/django/drf/permission-classes` | `check` lint: `forge_helper python drf_permissions.py` (§13.4): unless the settings module Django runs with (`DJANGO_SETTINGS_MODULE` set in `manage.py`, `wsgi.py` or `asgi.py`, star imports followed; else a `settings.py` / `settings/` outside tests) sets `DEFAULT_PERMISSION_CLASSES` to a non-empty list without `AllowAny`, a class inheriting a DRF view or viewset (directly or through project classes and mixins of any kind) gets a non-empty `permission_classes` or `get_permissions` from itself or a project ancestor, and an `@api_view` function has `@permission_classes`; `permission_classes = []` declares nothing; findings on the class or function line |
| `python/django/drf/serializer-fields` | `ast-grep` forbid: `fields = "__all__"` in the `Meta` of a class whose bases name a `Serializer` |
| `python/django/drf/no-allowany-write` | `ast-grep` forbid: `permission_classes` naming `AllowAny` in a `ModelViewSet`, a Create/Update/Destroy generic view or model mixin, or a class with `post`/`put`/`patch`/`delete`/`create`/`update`/`partial_update`/`destroy`; `@permission_classes([AllowAny])` on an `@api_view` with a write method; `AllowAny` as `DEFAULT_PERMISSION_CLASSES` |

context.md: explicit serializer fields and `read_only_fields`; permission defaults; ask before opening a write endpoint.

### python/celery

Detected by `celery` (pypi).

| Rule | Targets |
|---|---|
| `python/celery/bounded-retries` | `ast-grep` forbid: `max_retries=None` in a `shared_task(…)` / `<x>.task(…)` call or a `.retry()`, `.apply_async()`, `.signature()`, `.set()` call; `"max_retries": None` inside `retry_kwargs=`; a `max_retries = None` class attribute of a class whose bases name `Task` |
| `python/celery/no-result-get-in-task` | `ast-grep` forbid: inside a task, `.get()`, `.wait()`, `.join()` on `.delay()` / `.apply_async()` / `AsyncResult()` results (directly, through a variable of the function, or as the variable of a `for` loop or comprehension over them) or on a called `group` / `chord` / `chain`; `disable_sync_subtasks=False` anywhere |

context.md: ids as arguments, idempotent tasks queued from `transaction.on_commit()`, bounded retries, no waiting on results inside tasks.

### python/library

Detected by `library_any: [pypi]` (§13.4).

| Rule | Targets |
|---|---|
| `python/library/all-exports` | `ast-grep` no-increase, not in tests: in a module with `__all__`, top-level function and class names without a leading underscore that `__all__` does not list |
| `python/library/deprecation-shim` | `check` lint: `forge_helper python public_api.py` (§13.4), `when_changed`: `**/*.py`, `pyproject.toml` |

context.md: `__all__` is the public API; removals and renames warn with `DeprecationWarning` until a major version.

### typescript/library

Detected by `library_any: [npm]` (§13.4).

| Rule | Targets |
|---|---|
| `typescript/library/deprecation-shim` | `check` lint: `forge_helper bun public_api.mjs removed` (§13.4), `when_changed`: TypeScript and JavaScript files, `package.json` |

context.md: entry points are the public API; renamed exports stay as `@deprecated` aliases until a major version.

### typescript/bun

| Rule | Targets |
|---|---|
| `typescript/bun/lock-sync` | `check` lint: `run: bun install --frozen-lockfile`, `when_changed: [package.json]` |
| `typescript/bun/no-other-pm` | `guard` ask: `npm install`, `npm i`, `npm ci`, `pnpm`, `yarn` |
| `typescript/bun/release-age` | `check` lint: `forge_bun_cooldown`: `bunfig.toml` `[install] minimumReleaseAge` ≥ 259200; `when_changed`: `bunfig.toml`, `package.json`, `bun.lock` |
| `typescript/bun/trusted-deps` | `check` lint: `forge_trusted_deps`: no `trustedDependencies` entry added to a `package.json` since the base (with `bun`) |
| `typescript/bun/test` | `check` test: `bun test` on the tracked test files that import from `bun:test` (nothing when there are none, so a Vitest suite is not run by Bun) |
| `typescript/bun/no-new-skip` | `ast-grep` no-increase on test files (`*.test.*`, `*_test.*`, `*.spec.*`, `*_spec.*`) importing `bun:test`: `it`/`test`/`describe` `.skip`, `.todo`, `.only`, `.failing`, `.if`, `.skipIf`, `.todoIf` |
| `typescript/bun/no-fewer-expects` | `ast-grep` no-decrease on the same files: `expect($$$)` |
| `typescript/bun/no-mock-increase` | `ast-grep` no-increase on the same files: `mock(…)`, `mock.module(…)`, `spyOn(…)` (also under an import alias), `jest.fn/mock/spyOn`, `vi.fn/mock/spyOn` |
| `typescript/bun/no-test-edits` | `check` lint: `forge_test_edits` on the in-scope files that import `bun:test` (§10.5) |
| `typescript/bun/no-snapshot-update` | `guard` ask: `bun test` with `-u` or `--update-snapshots` |

context.md: `bun:test` tests run with `bun test`, what the integrity rules reject in them, and "If an existing test looks wrong, stop and ask; never edit it to make the code pass."

### typescript/nextjs

| Rule | Targets |
|---|---|
| `typescript/nextjs/no-pages-data` | `ast-grep` forbid in `app/**`, `src/app/**`: `getServerSideProps`, `getStaticProps` exports |
| `typescript/nextjs/no-next-router` | `ast-grep` forbid: `import $$$ from "next/router"` |
| `typescript/nextjs/proxy-not-middleware` | `ast-grep` forbid: any code in `middleware.ts` / `src/middleware.ts` (file renamed to `proxy.ts` in Next.js 16) |
| `typescript/nextjs/revalidate-tag-profile` | `ast-grep` forbid: `revalidateTag($A)` with one argument |
| `typescript/nextjs/no-legacy-image-config` | `ast-grep` forbid: imports of `next/legacy/image`; a `domains` key inside `images` |
| `typescript/nextjs/no-public-secret-env` | `ast-grep` forbid: `process.env.NEXT_PUBLIC_*` names containing `SECRET`, `PRIVATE`, `PASSWORD`, `SERVICE_ROLE` or `DATABASE_URL` |

context.md: Next.js 16 App Router: async request APIs, Server Components by default, `proxy.ts`, `revalidateTag(tag, profile)` / `updateTag(tag)`, `images.remotePatterns`, no `next lint`, Server Actions and route handlers are public endpoints (guidance only: validating input and checking the caller cannot be decided from syntax, so no rule checks it), no secrets in `NEXT_PUBLIC_`.

### typescript/nextjs/react

| Rule | Targets |
|---|---|
| `typescript/nextjs/react/client-hooks` | `ast-grep` forbid in `app/**`, `src/app/**`: client-only hooks (`useState`, `useEffect`, `useRef`, `useContext`, …) in a file without `'use client'`, except inside a custom hook (a function named `use…`) |
| `typescript/nextjs/react/no-server-in-client` | `ast-grep` forbid in `app/**`, `src/app/**`, `components/**`, `src/components/**` (tsx): a file starting with `'use client'` that imports `server-only`, `node:*`, `fs`, `child_process`, `@prisma/client`, `@prisma/adapter-*` or a `generated/prisma` path |

context.md: "React 19 in the App Router: components are Server Components unless their file starts with `'use client'`. State, effects, context, refs and event handlers need a Client Component; keep `'use client'` at the leaves. Server Actions are async functions marked `'use server'`."

### typescript/react

Detected by `react >= 19` (npm); a sibling of `nextjs` and `astro`, so it applies to every React setup.

| Rule | Targets |
|---|---|
| `typescript/react/no-forward-ref` | `tool` biome `lint/suspicious/noReactForwardRef`; `ast-grep` forbid calls to `forwardRef` / `React.forwardRef` (tsx) |
| `typescript/react/no-prop-types` | `ast-grep` forbid: `$C.propTypes = $V`, `$C.defaultProps = $V` |
| `typescript/react/use-action-state` | `ast-grep` forbid: `useFormState` imported from `react-dom` |
| `typescript/react/no-dangerous-html` | `tool` biome `lint/security/noDangerouslySetInnerHtml`; `ast-grep` forbid the `dangerouslySetInnerHTML` attribute |
| `typescript/react/hooks-rules` | `tool` biome `lint/correctness/useHookAtTopLevel`, `lint/correctness/useExhaustiveDependencies` |
| `typescript/react/stable-keys` | `tool` biome `lint/suspicious/noArrayIndexKey` |
| `typescript/react/no-nested-components` | `tool` biome `lint/correctness/noNestedComponentDefinitions` |
| `typescript/react/a11y` | `tool` biome `lint/a11y/useAltText`, `lint/a11y/useButtonType` (snippet: the whole `a11y` group at error) |
| `typescript/react/no-deprecated-types` | `tool` biome `lint/suspicious/noDeprecatedImports`, on imports React's types mark `@deprecated` (`ElementRef`, `MutableRefObject`, `LegacyRef`); Biome reads the tags from real `node_modules` directories only (not pnpm's symlinks) |

context.md: React 19: `ref` is a prop (no `forwardRef`), `useActionState` and `act` from `react`, no `propTypes` / `defaultProps`, `createRoot` instead of `ReactDOM.render`.

### typescript/astro

| Rule | Targets |
|---|---|
| `typescript/astro/check` | `check` typecheck: `run: astro check`, scope `**/*.astro`, `**/*.ts`, `**/*.tsx` |
| `typescript/astro/no-set-html` | `grep` forbid in `**/*.astro`: `set:html=` |

context.md: Astro renders on the server to static HTML; `astro check`; content collections in `src/content.config.ts` with `glob()` / `file()` loaders and `entry.id`; no `Astro.glob()`; `<ClientRouter />`.

### typescript/astro/react

context.md: "React components in Astro render to static HTML; they become interactive only with a `client:*` directive. Hooks and event handlers run only in hydrated islands. Props passed to a hydrated island must be serializable."

### typescript/prisma

| Rule | Targets |
|---|---|
| `typescript/prisma/validate` | `check` migration: `run: prisma validate`, `when_changed: [prisma/schema.prisma]` |
| `typescript/prisma/migration-present` | `check` migration: `run: prisma migrate diff --from-migrations prisma/migrations --to-schema prisma/schema.prisma --exit-code`, `when_changed: [prisma/schema.prisma]`, `requires_local_db: true`; skipped with a message if `prisma.config.ts` has no `shadowDatabaseUrl`; passes in a db push workflow (no `prisma/migrations` directory and `prisma db push` in `package.json`) |
| `typescript/prisma/no-reset` | `guard` deny: `prisma migrate reset`, `prisma db push --force-reset`, `prisma db push --accept-data-loss` |
| `typescript/prisma/db-push` | `guard` ask: `prisma db push`, `prisma migrate deploy`, `prisma db execute` |
| `typescript/prisma/generated-client-import` | `ast-grep` forbid: `import $$$ from "@prisma/client"` |
| `typescript/prisma/no-middleware` | `ast-grep` forbid: `$C.$use($$$)` |
| `typescript/prisma/no-raw-unsafe` | `ast-grep` forbid: `$C.$queryRawUnsafe($$$)`, `$C.$executeRawUnsafe($$$)` |
| `typescript/prisma/no-raw-interpolation` | `ast-grep` forbid: a call `$CALL` of `raw(…)` / `<x>.raw(…)` whose argument is a template with `${}`, a `+` concatenation or a variable declared from one (the `drizzle/no-sql-raw` matcher), where `CALL` is the runtime's `Sql` (`@prisma/client`, `@prisma/client-runtime-utils`): the generated client's path and import names never matter |
| `typescript/prisma/no-unscoped-bulk-write` | `ast-grep` advise: `.deleteMany(…)` / `.updateMany(…)` without an object argument holding `where`, where the call is the runtime's `PrismaPromise` (`@prisma/client`; generated delegates are project code) |
| `typescript/prisma/client-generator` | `check` lint: no `provider = "prisma-client-js"` in tracked `*.prisma` files, `when_changed: [**/*.prisma]` |
| `typescript/prisma/migration-lint` | `check` migration: changed `prisma/migrations/*/migration.sql` linted with Squawk (§13.1); `--all`: the newest migration; `when_changed: [prisma/migrations/**]` |
| `typescript/prisma/no-network-in-transaction` | `ast-grep` forbid: inside a function passed to `.$transaction(…)`, a call to the global `fetch` / `globalThis.fetch` / `$fetch`, or to a name (or a method of a name) imported from `axios`, `got`, `ky`, `ofetch`, `node-fetch` or `undici` |
| `typescript/prisma/destructive-migration` | `check` migration: `DROP COLUMN` / `DROP TABLE` statements (not `--` or `/* */` comments) in the changed `prisma/migrations/*/migration.sql` (`--all`: the newest), read by `forge_prisma_drops`, which follows each table's columns through every `migration.sql`: a SQLite redefinition (`CREATE TABLE "new_X"`, `DROP TABLE "X"`, `RENAME TO "X"`) is reported only for the columns `new_X` lacks (`… (redefinition drops column "C")`), and not when no earlier migration creates `X`; printed `file:line: <statement>`; a line found in `forge.yaml` (an `accept` entry) passes; `when_changed: [prisma/migrations/**]` |

context.md: Prisma 7: `prisma-client` generator with `output`, driver adapters, `prisma.config.ts` (no automatic `.env`), `prisma db seed`, `$extends` (no `$use`), tagged `$queryRaw` only (no `Prisma.raw()` from a template); no network calls in interactive transactions; migrations with schema changes (not under db push); drops accepted in `forge.yaml`.

### typescript/vitest

| Rule | Targets |
|---|---|
| `typescript/vitest/run` | `check` test: `run: vitest run`, `changed_run: vitest run --changed`; timeout 300 s |
| `typescript/vitest/no-new-skip` | `ast-grep` no-increase on test files: `it.skip`, `test.skip`, `describe.skip`, `it.todo`, `test.todo`, `describe.todo`, `$X.skipIf`, `$X.runIf`, `$X.only` |
| `typescript/vitest/no-fewer-expects` | `ast-grep` no-decrease on test files: `expect($$$)` |
| `typescript/vitest/no-test-env-branch` | `ast-grep` forbid on non-test files: `process.env.VITEST`, `process.env.NODE_ENV === "test"` |
| `typescript/vitest/no-tautological-expect` | `ast-grep` forbid on test files: `expect(<literal>).$M(…)` (`true`, `false`, `null`, `undefined`, a number or a string) |
| `typescript/vitest/no-snapshot-update` | `guard` ask: `vitest` with `-u` or `--update` |
| `typescript/vitest/no-workspace-file` | `check` lint: no tracked `vitest.workspace.*` (Vitest 4 ignores it), `when_changed: [vitest.*]` |
| `typescript/vitest/no-pass-with-no-tests` | `guard` ask: `vitest … --passWithNoTests` |
| `typescript/vitest/config-file` | `permission` ask: `Edit(**/vitest.config.*)` |

| `typescript/vitest/no-mock-increase` | `ast-grep` no-increase on test files: `vi.mock`, `vi.doMock`, `vi.spyOn`, `vi.fn` calls, also through `import { vi as <alias> }`; `vi.stubEnv` / `vi.stubGlobal` are not counted |
| `typescript/vitest/no-test-edits` | `check` lint: `forge_test_edits` on `**/*.test.*`, `**/*.spec.*`, `**/__tests__/**`, `**/__mocks__/**`, `**/test-utils.*`, `**/test-helpers.*`, `**/vitest.setup.*`, not `e2e/**`; edits ask (§10.5); `review` as pytest's |
| `typescript/vitest/untested-change` | `check` lint, advisory (§10.7): `forge_untested_change` on non-test TypeScript and JavaScript files (not `*.d.ts`, `*.config.*`, `e2e/**`) |
| `typescript/vitest/snapshot-stale` | `check` lint, advisory (§10.7, `forge_once`): `forge_snapshot_stale`, Stop only (`--turn`): a changed non-test source file whose added lines hold a string or template literal in an expression (not an import or export source, a type annotation, a literal type, an interface or a type alias) or an object literal key (`pair`, shorthand property; ast-grep, `FORGE_SNAP_VALUES`), imported directly (relative import) by a test that uses `toMatchSnapshot`, `toMatchFileSnapshot`, `toMatchInlineSnapshot` or golden files (`testdata/`, `golden`), while no `.snap`, `__snapshots__/`, `testdata/` or golden file changed in the turn (a test using only inline snapshots counts as updated when it changed). It blocks once per turn, then prints `accepted` and passes (`forge_once`) |

context.md: Vitest 4 `test.projects` and `maxWorkers`; assertions are never weakened to make a test pass; "When the task changes behaviour an existing test asserts, update that test in place with Edit or Write (keep its name; Forge asks for approval) and say so in your summary. Otherwise never edit an existing test to make the code pass."

### typescript/playwright

| Rule | Targets |
|---|---|
| `typescript/playwright/run` | `check` test: `run: playwright test --reporter=line`, `when_changed: [e2e/**, playwright.config.*]` |
| `typescript/playwright/no-new-skip` | `ast-grep` no-increase on `e2e/**`: `test.skip`, `test.fixme`, `test.fail`, `test.only`, `test.describe.skip`, `test.describe.fixme`, `test.describe.only` |
| `typescript/playwright/no-wait-for-timeout` | `ast-grep` forbid in `e2e/**`, `**/*.spec.*`: `$P.waitForTimeout($$$)` |
| `typescript/playwright/locators` | `ast-grep` forbid in `e2e/**`, `**/*.spec.*`: `$`, `$$`, `$eval`, `$$eval`, `waitForSelector`, `waitForNavigation` calls |
| `typescript/playwright/no-snapshot-update` | `guard` ask: `playwright test` with `--update-snapshots` or `-u` |
| `typescript/playwright/config-file` | `permission` ask: `Edit(**/playwright.config.*)` |

| `typescript/playwright/axe` | `check` lint: when `package.json` names `@axe-core/playwright`, a tracked `e2e/*` or `*.spec.*` file uses `AxeBuilder` |
| `typescript/playwright/no-test-edits` | `check` lint: `forge_test_edits` on `e2e/**`; edits ask (§10.5); `review` as pytest's |

context.md: locators (`getByRole`, `getByLabel`, `getByText`, `getByTestId`) and web-first assertions; never `waitForTimeout`; `@axe-core/playwright` for accessibility scans; "When the task changes behaviour an existing test asserts, update that test in place with Edit or Write (keep its name; Forge asks for approval) and say so in your summary. Otherwise never edit an existing test to make the code pass."

### typescript/zod

| Rule | Targets |
|---|---|
| `typescript/zod/v4-string-formats` | `ast-grep` forbid: a string-format method (`email`, `uuid`, `url`, `datetime`, …) called on a chain rooted at `z.string()` |
| `typescript/zod/error-param` | `ast-grep` forbid: a `message`, `required_error`, `invalid_type_error` or `errorMap` key in an object passed to a call on a `z.` chain |

context.md: Zod at boundaries with `z.infer`; Zod 4 top-level formats, the single `error` parameter, `z.strictObject` / `z.looseObject`, `error.issues`, `z.treeifyError`.

### docs

Detected by `**/docs/**/*.{md,mdx,rst}`, `**/doc/**/*.{md,mdx,rst}`, `README*`, `packages/*/README*`. Not tied to a language: it reads Python, TypeScript and JavaScript sources.

| Rule | Targets |
|---|---|
| `docs/changed-identifiers` | `check` lint, advisory (§10.7): `forge_docs_mentions` on non-test Python, TypeScript and JavaScript files |

### typescript/drizzle

Detected by `drizzle.config.*` or the `drizzle-orm` / `drizzle-kit` dependency. The checks read `schema`, `out` (default `./drizzle`) and `dialect` from `drizzle.config.*` (string values, arrays, globs and directories; values computed at run time are not read).

| Rule | Targets |
|---|---|
| `typescript/drizzle/migration-present` | `check` migration: `forge_drizzle_migration_present`: a changed schema file whose content reaching the database changed (blank lines, comments, imports, type exports, `relations()` / `defineRelations()` calls aside) needs, among the changed files, a `.sql` in `out` plus `meta/_journal.json` and a `meta/*_snapshot.json`, or (drizzle-kit 1.x) `<out>/<name>/migration.sql` and a `snapshot.json`; not with `--all` |
| `typescript/drizzle/applied-migrations` | `check` migration: `forge_drizzle_applied`: no `.sql` or snapshot in `out` that exists in the base is modified or deleted |
| `typescript/drizzle/migration-lint` | `check` migration: with `dialect: "postgresql"`, changed `.sql` files in `out` linted with Squawk (§13.1); `--all`: the newest one; `when_changed: [**/*.sql]` |
| `typescript/drizzle/meta-files` | `permission` deny: `Edit(**/meta/_journal.json)`, `Edit(**/meta/*_snapshot.json)` |
| `typescript/drizzle/no-push` | `guard` ask: `drizzle-kit push` |
| `typescript/drizzle/no-push-force` | `guard` deny: `drizzle-kit push … --force` (auto-approves data-loss statements) |
| `typescript/drizzle/write-with-where` | `ast-grep` forbid: `$DB.delete(…)` / `$DB.update(…)` with no `.where` further along its method chain, where `DB` is a Drizzle database (`PgDatabase`, `MySqlDatabase`, `BaseSQLiteDatabase`, `SingleStoreDatabase`, `GelDatabase`; transactions extend them) |
| `typescript/drizzle/no-sql-raw` | `ast-grep` forbid: `sql.raw(…)` (or `<alias>.raw(…)` with `import { sql as <alias> } from "drizzle-orm"`) whose argument is a template with `${}` or a `+` concatenation, or a variable declared from one in an enclosing block or the module |

context.md: schema, `out` and `dialect` in `drizzle.config.*`; `drizzle-kit generate` after a schema change, its files committed together and never written by hand; applied migrations are not edited; `push` needs confirmation, `push --force` is refused; deletes and updates have a `where`; the `sql` tag binds values.

### typescript/trpc

| Rule | Targets |
|---|---|
| `typescript/trpc/input-validate` | `ast-grep` forbid: `.input(…)` whose argument is `z.any()`, `z.unknown()`, `z.custom()` / `z.custom<T>()` without a check, `z.record(…)` of `z.any()` / `z.unknown()`, `(v) => v as T` or `(v: T) => v` |
| `typescript/trpc/trpc-error` | `ast-grep` forbid: `throw new Error(…)` or `throw Error(…)` inside a function passed as the first argument of `.query()`, `.mutation()` or `.subscription()`, in a file importing `@trpc/server` or when that function destructures `ctx` or `input`; not inside a `try` block |

context.md: `.input(<schema>)` and what is rejected; `TRPCError` with a `code` in procedures.

### typescript/hono

| Rule | Targets |
|---|---|
| `typescript/hono/validate-input` | `ast-grep` forbid, in files importing `hono`, `hono/*` or `@hono/*`: `.json()`, `.parseBody()`, `.formData()`, `.query()`, `.queries()` on `$C.req` or on a `req` destructured from the handler's parameter (`({ req })`, `({ req: r })`), unless the call (or its `await`) is passed straight to `X.parse` / `safeParse` / `parseAsync` / `safeParseAsync` (not on `JSON`, `Date`, …) or as the second argument of `parse(Schema, …)` / `v.parse(…)`; `c.req.param()` is not checked |
| `typescript/hono/cors-credentials-wildcard` | `ast-grep` forbid: `cors({…})` with `credentials: true` and `origin: "*"` or an `origin` function that returns its first parameter (as its body, with a `??` / `||` fallback, or in a `return`) |

context.md: `zValidator` / `validator()` middleware and `c.req.valid()`; which raw reads are rejected; `cors()` with credentials names its origins.

### typescript/react-router

Detected by `@react-router/dev >= 7` (framework mode).

No rules: whether an action validates its input and checks the caller cannot be decided from syntax (rules that guessed it from helper names were removed).

context.md: route module `loader` / `action` with `Route.*Args` types; actions are public endpoints (guidance, not checked); `data()` and `redirect()`.

### typescript/vue

Detected by `vue >= 3`. ESLint (`eslint-plugin-vue`) is not audited: `tool` targets are Biome, ruff, ty and tsc; the `.vue` rules are `check` and `grep` targets instead.

| Rule | Targets |
|---|---|
| `typescript/vue/typecheck` | `check` typecheck: `forge_exec_tool vue-tsc --noEmit` (`--build` when `tsconfig.json` has `references`); skipped while a Nuxt `tsconfig.json` points at a missing `.nuxt/`. A tool whose name has a `-` gets no wrapper function (not a POSIX function name) and is run as `forge_exec_tool <name>` |
| `typescript/vue/v-for-key` | `check` lint: `forge_vue_vfor_keys`: in `.vue` templates (comments, `<script>`, `<style>` skipped; quotes respected), an element other than `<template>` with `v-for` and no `:key` / `v-bind:key`, reported at the `v-for` line |
| `typescript/vue/no-v-html` | `grep` forbid in `**/*.vue`: `v-html=`, `:innerHTML=`, `v-bind:innerHTML=` |

context.md: `<script setup lang="ts">`, `vue-tsc` for `.vue` types, `:key` with `v-for`, no `v-html`, eslint-plugin-vue not run by Forge.

### typescript/vue/nuxt

| Rule | Targets |
|---|---|
| `typescript/vue/nuxt/server-validate` | `ast-grep` forbid in `server/**`: `readBody(…)`, `readFormData(…)`, `readMultipartFormData(…)`, `getQuery(…)`, `getRouterParams(…)`, `getRouterParam(…)` (h3's `readValidatedBody`, `getValidatedQuery`, `getValidatedRouterParams` instead), unless the call (or its `await`) is passed straight to `X.parse` / `safeParse` / `parseAsync` / `safeParseAsync` (not on `JSON`, `Date`, `Number`, `URL`) |
| `typescript/vue/nuxt/no-public-secret` | `ast-grep` forbid in `nuxt.config.*`: a key under `runtimeConfig.public` whose name contains `secret`, `private`, `password` / `passwd`, `credential`, `token`, `api_key` / `apiKey`, `access_key`, `auth_key`, `service_role`, `dsn` or `database_url` (case-insensitive), unless it also says `publishable`, `public`, `anon` or `site_key` |

context.md: validated h3 readers in server routes; `runtimeConfig.public` reaches the browser; `vue-tsc` after `nuxt prepare`.

### typescript/openapi

Detected by `openapi-typescript`, `orval` or `@hey-api/openapi-ts`. Both checks run only outside `--all`, and only when a tracked `package.json` has a script that runs one of the generators. An OpenAPI document is a changed `.json` / `.yaml` / `.yml` file with an `openapi:` or `swagger:` version in its first 4 KiB; a generated file is one whose first 15 lines say `auto-generated by openapi-typescript`, `auto-generated by @hey-api/openapi-ts` or `Generated by orval` (other generators' "do not edit" headers do not count).

| Rule | Targets |
|---|---|
| `typescript/openapi/regenerate` | `check` lint: `forge_openapi_sync regenerate`: a changed OpenAPI document and no changed generated file fail |
| `typescript/openapi/no-generated-edits` | `check` lint: `forge_openapi_sync hand-edits`: a changed generated file fails when no OpenAPI document, `orval.config.*`, `openapi-ts.config.*` or `redocly.*` changed, nor a `package.json` line naming the generator (its script or dependency version), and the repository tracks an OpenAPI document |

context.md: the client comes from the document through a generator script; the two change together, generated files are never edited by hand.

### typescript/nestjs

Detected by `@nestjs/core` or `@nestjs/common`. Decorators are matched as the bindings imported from their package (`import { Body as B } from "@nestjs/common"` included), never by a name alone.

| Rule | Targets |
|---|---|
| `typescript/nestjs/validation-pipe-whitelist` | `ast-grep` forbid: `new $N(…)` without `whitelist: true` in its object argument, where `N` is `@nestjs/common`'s `ValidationPipe` |
| `typescript/nestjs/validation-pipe-forbid-non-whitelisted` | `ast-grep` advise: the same without `forbidNonWhitelisted: true` |
| `typescript/nestjs/dto-is-class` | `ast-grep` forbid: a parameter decorated `@Body()` / `@Query()` / `@Param()` (from `@nestjs/common`, no arguments) with no type, `any`, `unknown`, `object`, an object type literal or `Record<…>` |
| `typescript/nestjs/validate-nested-type` | `ast-grep` forbid: a property decorated with class-validator's `ValidateNested` and no class-transformer `Type` decorator |
| `typescript/nestjs/http-exception` | `ast-grep` advise: `throw new Error(…)` in a method of a class decorated with `@nestjs/common`'s `Controller`, not inside a `try` block |

context.md: `ValidationPipe` with `whitelist` and `forbidNonWhitelisted`; DTO classes on `@Body()` / `@Query()` / `@Param()`; `@ValidateNested()` with `@Type()`; `HttpException` subclasses in controllers.

### typescript/kysely

| Rule | Targets |
|---|---|
| `typescript/kysely/no-sql-raw` | `ast-grep` forbid: a call `$CALL` of `<x>.raw(…)` / `<x>.lit(…)` whose argument is a template with `${}`, a `+` concatenation or a variable declared from one (the `drizzle/no-sql-raw` matcher), where `CALL` is Kysely's `RawBuilder` |
| `typescript/kysely/write-with-where` | `ast-grep` advise: `$DB.deleteFrom(…)` / `$DB.updateTable(…)` whose method chain reaches `execute()` / `executeTakeFirst()` / `executeTakeFirstOrThrow()` with no `where…` on the way, where `DB` is Kysely's `Kysely` or `Transaction` |

context.md: the `sql` tag binds values, `sql.ref()` / `sql.id()` for names, no `sql.raw()` / `sql.lit()` from a template; deletes and updates have a `where`.

### typescript/typeorm

| Rule | Targets |
|---|---|
| `typescript/typeorm/no-synchronize` | `ast-grep` forbid: a pair `synchronize: true` / `dropSchema: true` in the object argument of `new $D(…)` where `D` is TypeORM's `DataSource`, of `TypeOrmModule.forRoot(…)`, or in an object a `TypeOrmModule.forRootAsync` `useFactory` returns (`TypeOrmModule` the binding imported from `@nestjs/typeorm`) |
| `typescript/typeorm/synchronize-expression` | `ast-grep` advise: the same keys set from an expression (not `true` / `false`) |
| `typescript/typeorm/no-raw-query-interpolation` | `ast-grep` forbid: `$R.query(…)` whose first argument is a template with `${}`, a `+` concatenation or a variable declared from one, where `R` is `DataSource`, `EntityManager`, `QueryRunner` or `Repository` |
| `typescript/typeorm/no-qb-where-interpolation` | `ast-grep` forbid: `where` / `andWhere` / `orWhere` / `having` / `andHaving` / `orHaving` / `orderBy` / `addOrderBy` with such a first argument on a `SelectQueryBuilder`, `UpdateQueryBuilder`, `DeleteQueryBuilder`, `SoftDeleteQueryBuilder` or `WhereExpressionBuilder` |
| `typescript/typeorm/schema-sync-guard` | `guard` deny: `typeorm` / `typeorm-ts-node-esm` / `typeorm-ts-node-commonjs` `schema:sync`, `schema:drop` |
| `typescript/typeorm/migration-revert` | `guard` ask: the same bins with `migration:revert` |

context.md: migrations, never `synchronize` / `dropSchema`; parameters in `.query()` and in query builder conditions.

### typescript/mongoose

| Rule | Targets |
|---|---|
| `typescript/mongoose/no-request-filter` | `ast-grep` forbid: `$M.<op>($R.body \| query \| params, …)` or as the second argument, `<op>` one of `find`, `findOne`, `findOneAndUpdate`, `findOneAndDelete`, `findOneAndReplace`, `updateOne`, `updateMany`, `deleteOne`, `deleteMany`, `countDocuments`, `replaceOne`, where `M` is a Mongoose `Model` or `Query` and `R` an Express `Request` or a `FastifyRequest` |
| `typescript/mongoose/no-where-operator` | `ast-grep` forbid: a `$where` key in the arguments of a call on `$M`, or `$M.$where(…)`, where `M` is a Mongoose `Model` or `Query` |

context.md: request input validated or `sanitizeFilter()`ed before a filter; no `$where`.

### typescript/temporal

| Rule | Targets |
|---|---|
| `typescript/temporal/workflow-determinism` | `ast-grep` forbid, in a file with a value import of `@temporalio/workflow`: a value import or `require()` of `node:*`, `fs`, `fs/promises`, `path`, `os`, `crypto`, `http`, `https`, `net`, `tls`, `dns`, `child_process`, `worker_threads`, `cluster`, `@temporalio/activity`, `@temporalio/client` or `@temporalio/worker`; `crypto.randomUUID()` / `crypto.getRandomValues()`. A project module holding activities imported by value is not detected (which module holds activities is not known without guessing) |

context.md: the workflow sandbox; activities through `proxyActivities` with `import type`; `uuid4()`.

### github-actions

Detected by `.github/workflows/*.yml` or `*.yaml`.

| Rule | Targets |
|---|---|
| `github-actions/zizmor` | `check` lint: `forge_zizmor` (§13.2) on changed workflows and actions, failing on the `unpinned-uses`, `template-injection`, `dangerous-triggers`, `excessive-permissions` and `artipacked` audits |

context.md: actions pinned to a commit SHA, `permissions: {}` then per job, `persist-credentials: false`, no `${{ }}` inside `run:`, no `pull_request_target` / `workflow_run` on untrusted code.

### docker

Detected by `**/Dockerfile`, `**/Dockerfile.*`, `**/*.Dockerfile`, `**/Containerfile` or a compose file (a repository may use Compose only for a local database). Rules are `check` lint targets on the Dockerfiles of the change (every tracked one with `--all`).

| Rule | Targets |
|---|---|
| `docker/pin-base-image` | `forge_hadolint DL3006 DL3007` (§13.3) |
| `docker/copy-not-add` | `forge_hadolint DL3020` |
| `docker/exec-form` | `forge_hadolint DL3025` |
| `docker/workdir` | `forge_hadolint DL3045` |
| `docker/pipefail` | `forge_hadolint DL4006` (DL3008, pinned apt versions, is not enforced: they disappear from mirrors) |
| `docker/non-root-user` | `forge_docker_nonroot`: the final stage has a `USER` other than `root` / `0`, set in it or inherited from the named stage it builds on; base images under `cgr.dev/` or `dhi.io/`, or named `*nonroot*`, `*unprivileged*`, `*rootless*`, are exempt (hadolint's DL3002 misses a Dockerfile with no `USER`) |
| `docker/no-secret-arg-env` | `forge_docker_secrets`: no `ARG` / `ENV` named like `*PASSWORD*`, `*SECRET*`, `*TOKEN*`, `*API_KEY*`, `*PRIVATE_KEY*`, `*CREDENTIALS*` (`*_FILE` is fine); use `RUN --mount=type=secret` |
| `docker/locked-install` | `forge_docker_locked`: in `RUN` (continuations joined, commands split on `&&`, `\|\|`, `;`): no `npm install` / `npm i` (except `-g`), `bun install` with `--frozen-lockfile` and no `bun add`, `uv sync` with `--locked` or `--frozen` and no `uv add`, `pnpm install` with `--frozen-lockfile`, `yarn` with `--immutable` or `--frozen-lockfile` |
| `docker/dockerignore` | `forge_docker_ignore`: each Dockerfile has a `.dockerignore` (`<Dockerfile>.dockerignore`, its directory's, or the root's) that excludes `.env` files |

Structural findings (`non-root-user`, `dockerignore`) are reported at line 0, so they count whenever the Dockerfile changed (removing a `USER` adds no line). context.md: multi-stage builds, a non-root final stage, lock-file installs with cache mounts, build secrets, pinned base images; the sandbox blocks docker unless the user lists it in `sandbox.excludedCommands`. Forge does not write that setting; `forge status` prints the line to add when the pack is active.

### docker/compose

Detected by `**/compose*.y*ml` or `**/docker-compose*.y*ml`. ast-grep rules use `scope.language: yaml` on the compose files.

| Rule | Targets |
|---|---|
| `docker/compose/config-valid` | `check` lint: `docker compose config --quiet --no-interpolate --no-env-resolution` on the root compose file (no `.env` is read) |
| `docker/compose/no-version-key` | `ast-grep` forbid: a top-level `version` key |
| `docker/compose/no-docker-sock` | `ast-grep` forbid: a scalar naming `/run/docker.sock` or `/var/run/docker.sock` |
| `docker/compose/no-host-root-mounts` | `ast-grep` forbid: under `volumes`, a short-syntax source or a long-syntax `source` of `/`, `/etc`, `/root`, `/home`, `~`, `$HOME` |
| `docker/compose/no-privileged` | `ast-grep` forbid: `privileged: true`; `network_mode`, `pid`, `ipc`, `userns_mode` or `uts` set to `host`; `cap_add` with `ALL` or `SYS_ADMIN`; a `security_opt` entry with `unconfined` |
| `docker/compose/datastore-loopback` | `ast-grep` forbid: a short-syntax `ports` entry publishing 5432, 3306, 6379, 27017, 9200, 5672, 11211 or 1433 that does not start with `127.0.0.1:` (or `localhost:`, `[::1]:`); Docker-published ports bypass ufw and firewalld. Long syntax is not checked |
| `docker/compose/no-inline-secrets` | `ast-grep` forbid: under `environment` or `args`, a secret-like key (map form) or `KEY=value` entry (list form) whose value is neither empty nor a `${VAR}` reference; `*_FILE` keys are fine |
| `docker/compose/healthy-depends` | `ast-grep` forbid: `depends_on` as a list (block or flow); the long form with `condition:` is required |
| `docker/compose/pinned-images` | `ast-grep` forbid: `image:` with `:latest` or no tag or digest, unless the service also has `build:`; `${VAR}` images are not checked |
| `docker/compose/env-not-committed` | `check` lint: `git ls-files` lists no `.env` or `.env.*` other than `.env.example`, `.env.sample`, `.env.template` |

context.md: `compose.yaml` + `compose.override.yaml`, healthchecks with `condition: service_healthy`, `develop.watch`, profiles, secrets, loopback-bound database ports, and that docker compose runs outside the sandbox, which makes these rules a real boundary.

### 13.1 Migration linting

`migration-lint` checks run [Squawk](https://squawkhq.com) (`squawk-cli`, pinned, through `bunx` or `uvx`, like ast-grep) on migration SQL and fail only on the changes that break the running version of the application during a deploy: `renaming-column`, `renaming-table`, `changing-column-type`, `adding-required-field`, `adding-not-nullable-field`, `ban-drop-database`. Squawk's other advice (lock and statement timeouts, `IF NOT EXISTS`, `CONCURRENTLY`, column types) is not enforced: Prisma and Alembic generate SQL that triggers it on every migration. Findings are printed `file:line:column` (from 1). When Squawk cannot run (offline, no `bunx` or `uvx`) or Alembic cannot render a revision, the check prints `forge:<id> skipped: …` and does not block. In CI (`--all`) only the newest migration is linted, so an accepted rename does not fail every later run.

### 13.2 zizmor

`forge_zizmor <audit>…` runs [zizmor](https://docs.zizmor.sh) pinned (`uvx zizmor@<pin>`), `--offline` (no GitHub token, no network audits), `--no-ignores` (inline `# zizmor: ignore` comments and ignore rules in its config do not apply; a rule is disabled in `forge.yaml`) and `--format github`, on the changed in-scope files (every tracked workflow and action with `--all`). Findings of the named audits print as `<path>:<line> <audit>: <message>` and are line-attributed like any lint check. When uvx is missing or zizmor cannot run (offline without a cached copy), the check prints `forge:<id> skipped: …` and does not block.

### 13.3 hadolint

`forge_hadolint <code>…` runs hadolint (pinned, from the `hadolint-bin` wheel through `uvx --from hadolint-bin==<pin>`) once per `check.sh` run on the Dockerfiles of the change, shared by the hadolint rules. Before running it, it hashes the binary and compares it with the upstream release's sha256 for the platform (embedded in Forge); a mismatch, an unknown platform, or no uvx prints `forge:<id> skipped: …` and does not block. It runs with `-c <file containing "ignored: []">` (the project's `.hadolint.yaml` ignores do not apply), `--disable-ignore-pragma` (inline `# hadolint ignore=` comments do not apply; a rule is disabled in `forge.yaml`), `--no-fail --no-color -f tty`; lines `path:line CODE level: message` of the named codes print as `path:line CODE: message`. `forge test` also checks the wheel's binary against the upstream checksum.

### 13.4 Libraries and project-run helpers

`detect.library_any` activates a pack when the project publishes a library to that ecosystem. The shape of a package is not enough (most uv workspace members and packaged services have a build backend; internal workspace packages declare `exports`), so detection also needs **evidence that it is published**; without it, add the pack in `forge.yaml`. Each `pyproject.toml` / `package.json` (the root's and each workspace package's) is judged on its own. **pypi:** `pyproject.toml` has `[project] name` and a `[build-system]` table (`uv init --lib`; `uv init` makes an application without one), no `[project.scripts]` or `[project.gui-scripts]` (`uv init --package`: a packaged application), no `Private ::` classifier, no `manage.py` in its package (for the root: outside the workspace packages), and is published: it has trove `classifiers` (PyPI metadata), or a CI workflow (`.github/workflows/*.yml`, `.gitlab-ci.yml`) uploads to PyPI (`pypa/gh-action-pypi-publish`, `uv`/`poetry`/`hatch`/`flit`/`pdm publish`, `twine upload`). **npm:** `package.json` is not `"private": true`, has a `name`, has `exports`, or `main` / `module` together with `types`, `typings` or `files` (`main` alone is `npm init`'s default), and is published: `publishConfig`, Changesets (`.changeset/config.json`), or a CI workflow running `npm`/`pnpm`/`yarn`/`bun publish`, `changesets/action` or `semantic-release`. A library that ships a CLI script is not detected; add the pack in `forge.yaml`.

`forge_helper <runtime> <script> [args]` runs `.forge/helpers/<script>` (written when an active check uses it, like the ratchet's) with the project's `python` or `bun`, as `<script> <FORGE_TMP> [args]`, where the change set (`changed`, `inbase`, `base/`) is; findings are `path:line message` (line 0: the file as a whole), attributed like lint diagnostics; a finding line found in `forge.yaml` is accepted; exit 3 prints `skipped:`.

- `drf_permissions.py`: §13 `python/django/drf/permission-classes`.
- `fk_index.py`: §13 `python/sqlalchemy/fk-index`: in each changed `.py` file outside `versions/`, the foreign key columns (a `ForeignKey(…)` argument of `mapped_column` / `Column`, a `ForeignKeyConstraint` in `__table_args__`; per class or `Table(…)`) not in the base copy that no index leads with: `index=True` / `unique=True`, the first `primary_key=True` column, or the first column of an `Index` (after its name), `UniqueConstraint` or `PrimaryKeyConstraint`. Reported at the column's line.
- `public_api.py`: for each changed module whose base copy has a literal `__all__`, each name that left it, or every name of a deleted module, must still be defined with a deprecation, read from the syntax tree: a `def` of that name decorated with `@deprecated` (`typing_extensions`, `warnings`) or calling `warnings.warn(…, DeprecationWarning | FutureWarning | PendingDeprecationWarning)` (or a subclass defined in the project, or a category parameter defaulting to one); a `class` with that decorator or such a call in `__init__` / `__new__` / `__init_subclass__` / `__post_init__`; an alias `name = deprecated(…)(…)`; or a module `__getattr__` that warns that way and names it (or reads a module-level table naming it). A project function that warns that way counts when the shim calls it or decorates with it. The word in a docstring or a comment does not count. A major version bump does not allow a removal by itself (the agent could make it): an intended removal is accepted in `forge.yaml`, which asks.
- `public_api.mjs removed`: the names the package's entry points (`exports`, `main`, `module`, `types`, with `dist/`, `lib/`, `build/`, … mapped to `src/` and `.js` / `.d.ts` to the source file; `export *` followed through relative modules) export in the base are still exported, a major version bump included (an intended removal is accepted in `forge.yaml`); reported at line 0 of the module that declared the name. A name that became an alias of another exported name (`export const old = renamed`, `export { renamed as old }`; it was not one in the base) needs a `/** … @deprecated … */` JSDoc block right before its export statement or specifier.

## 14. Acceptance tests

The test suite in `tests/spec/` is the executable form of this specification: one file per section (`@spec <number> · <title>` header), one requirement per test title. The documentation site renders those files with the JUnit results of the CI run (`bun run test:report` → `test-results/<platform>.xml`) and fails to build if a result cannot be placed on a test.

### 14.1 `forge test` and `bun run test:packs`

The same checks run on two sets of rules: `forge test` on a project's local rules (`.forge/rules/`), and the development entry point `bun run test:packs [packs-dir]` (`scripts/test-packs.ts`, not part of the published CLI; the spec suite and CI run it through `tests/spec/13-packs`) on every bundled pack and rule.

- Every pack and rule validates against its schema.
- ast-grep targets compile to `rule-tests/*.yml` (`valid` = `no_match`, `invalid` = `match`) and pass `ast-grep test --skip-snapshot-tests`. The number of loaded rules equals the number of rule files.
- Every guard target: `guard.sh` on each example wrapped in hook JSON (its `cwd` a directory holding the example's `files`, executable, and `.git`) returns the rule's decision for `match` and nothing for `no_match`, on macOS `sh` (bash 3.2), dash and BusyBox.
- Every grep target: `grep -E` matches a line of each `match` example and no line of a `no_match` example.
- The `hadolint-bin` wheel's binary for this platform has the upstream release's sha256 (when a rule uses hadolint and uvx is available).
- Every check target: each example runs in a temporary git repository holding the fixture's `head` files (committed), a `.forge/check.sh` generated with only that check, then the fixture's `files` written and staged, as a turn that started at the `head` fixture. `check.sh` runs as Stop runs it (`--turn`; `format` checks, which fix files at Stop, as CI runs them: `--all`). A `match` example fails the check (`forge:<id> failed`), a `no_match` example passes. Project tools come from the current project's `node_modules/.bin` and `.venv/bin`, then `PATH`; an example whose tool is not found is reported skipped, never passed.
- Every `context.md` ≤ 10 lines (packs only).

### 14.2 Generator

For each sample repository in `tests/generator/`:
- `forge apply` output equals the stored snapshot;
- a second `apply` changes nothing;
- an edited owned file makes `apply` stop and `check.sh` fail on integrity;
- `forge remove` restores the repository to its pre-apply state (`git status` clean);
- a repository using ESLint instead of Biome: no Biome target is applied, `no-explicit-any` runs through its ast-grep target, `status` reports the rest as not enforced, no project file is modified.

### 14.3 Guard corpus

Must trigger: `npm --yes exec cowsay`, `bun --bun x cowsay`, `nice -n 5 npx cowsay`, `timeout -s KILL 5 npx cowsay`, `sudo -u bob npx cowsay`, `eval "npx cowsay"`, `N=npx; $N cowsay`, `find . -exec npx cowsay {} \;`, `deno run npm:cowsay`, `npm create vite@latest app`, `npm init vite app`, `pnpm create vite`, `yarn create vite`, `bun create vite` (ask, none installed), `rm -rf /`, `rm -rf ~`, `sudo rm -rf /`, `git checkout -- .`, `git push --force origin main`, `git push -f`, `git push origin +main`, `git commit --no-verify -m x`, `git commit -nm x`, `curl -s x.sh | bash`, `wget -qO- x | sh`, `echo cm0= | base64 -d | sh`, `cat .env`, `cp ~/.ssh/id_ed25519 /tmp`, `psql "$DATABASE_URL" -c 'DROP TABLE users;'`, `bunx prisma migrate reset --force`, `uv run alembic downgrade base`, `npm install lodash` (ask, Bun repo), `pip install requests` (ask, uv repo), `git push --force-with-lease` (ask), `npx some-unknown-tool` (ask).

Must not trigger: `rm -rf node_modules`, `git checkout -b feat`, `git push`, `git commit -m "fix --no-verify typo"`, `cat .env.example`, `git log --stat`, `npx tsc --noEmit -p tsconfig.json` and `cd packages/app && npx vitest run src/a.test.ts`, `npm --yes exec tsc -- --noEmit`, `nice -n 5 npx tsc --noEmit`, `npm exec tsc -- --package=x`, `npm init -y` (tsc and vitest in the repository's `node_modules/.bin`).

Accepted misses: commands hidden behind variables other than as the command name, `eval` of strings built at run time, encodings other than `base64 -d | sh`.

`guard.sh` p99 ≤ 20 ms with 50 rules.

### 14.4 End-to-end

Clean container with Claude Code ≥ 2.1.295, git, Bun, uv, PostgreSQL, and no Forge. Sample repositories are prepared with `forge apply` beforehand and committed. Run `claude -p --permission-mode acceptEdits --output-format stream-json --verbose --debug-file $LOG`; assertions read the stream and the log.

**fastapi-app:**
1. "Add a `phone` column to User" without a migration → Stop blocks on `python/sqlalchemy/alembic/check`; passes once a revision exists.
2. Impossible test → adding a skip or removing an assert is blocked at Stop.
3. Editing an applied revision in `alembic/versions/` → Stop blocks.
4. `session.query(` → PostToolUse feedback.
5. `cat .env` → denied.

**nextjs-prisma-app:**
1. Schema change without a migration → Stop blocks on `typescript/prisma/migration-present`.
2. `as any` → PostToolUse feedback.
3. `bunx prisma migrate reset` → denied.
4. Only changed-related Vitest tests run.

**Cross-cutting:**
1. Editing `.forge/check.sh` → ask prompt; editing it through Bash (`sed`) → `check.sh` fails on integrity at Stop.
2. Three failed Stops → the 4th is allowed with the hand-over message.
3. A remote `DATABASE_URL` → migration checks print "skipped: no local database" and never block.
4. After `/compact`, SessionStart output is present again.
5. `guard.sh` deleted → PreToolUse blocked.
6. Bun absent in a TypeScript repo → hooks exit 0; SessionStart reports the missing runtime once.
## 15. Engine capabilities

A rule says **what** it needs to know about the code; the engine decides **how** to find it out. A capability is a question a rule asks declaratively in its YAML, answered by **one pinned, provisioned implementation per language** (§15.2). Packs and rules never name the tool that answers: replacing ty or TypeScript's server by another type checker changes the engine, never a rule.

- A capability MUST NOT decide on names the project chose (variables, receivers, helpers, parameters, free text). Literal conventions of a tool or library are allowed (a library's own API names, `Dockerfile`, `*_TOKEN` keys in `.env`), and each one is overridable in `forge.yaml` (§15.4).
- There is no fallback. A capability that cannot answer is reported with the rules it leaves unchecked, never taken as a pass and never replaced by a weaker heuristic (§15.1).

| Capability | Asked from | Answers | State |
|---|---|---|---|
| types | `where: {<VAR>: {type: …}}` on an `ast-grep` target | what the static type of the expression bound to a metavariable is declared as | implemented (§15.1–§15.3) |
| declared | `where: {<VAR>: {declared: …}}` | what a name refers to (an imported function or class, through aliases): `textDocument/definition` instead of `typeDefinition`, no supertypes | proposed (§15.6) |
| files | `@<role>` in `scope.files` and in check helpers | the paths of a file role: Forge's defaults, the tool's declarative config, `forge.yaml` | designed (§15.4) |
| tests | engine helpers of test checks | the tests a runner collects and the ones that failed, from the runner's own reports | proposed (§15.5) |

### 15.1 Types

```yaml
# packs/python/sqlalchemy/rules/no-query-api.yaml
enforce:
  - target: ast-grep
    mode: forbid                    # forbid or advise
    rule: {pattern: "$S.query($$$)"}
    where:
      S: {type: {package: sqlalchemy, name: [Session, scoped_session, AsyncSession, async_scoped_session]}}
examples:
  deps: [{ecosystem: pypi, name: sqlalchemy, version: "2.0.44"}]
  match: ["from sqlalchemy.orm import Session as Db\n\n\ndef first(db: Db):\n    return db.query(int).first()\n"]
  no_match: ["class Session:\n    def query(self, q: str) -> list[str]:\n        return [q]\n\n\ndef search(session: Session):\n    return session.query(\"text\")\n"]
```

`where` maps single metavariables of the rule (`$S`, not `$$$S`) to a condition: `{type: <spec>}` or `{not: <condition>}`. A spec has `package` and/or `module` (a string or a list; at least one: a name alone would be a guess), optionally `name` (a string or a list of identifiers) and `unknown: match | no-match` (default `no-match`).

**Semantics.** For each match, each condition is evaluated on the expression the metavariable is bound to; the match counts only when all of them hold.

1. **The expression's type** is what the language's type checker infers for exactly that expression, in the file as it is on disk: the engine shows the server, in memory only, the file with the expression bound to a fresh name (Python `(__forge_tN := (<expr>))`, TypeScript `({__forge_tN: (<expr>)}).__forge_tN`), asks `textDocument/typeDefinition` at that name, then shows it the file as it is. Any expression works (a name, an attribute, a call such as `sessionmaker(e)()`, a subscript). Nothing is written to the project.
2. **Declarations.** Each location returned is the declaration of the type, or of one member: a union or intersection gives one per member (`Session | None`: `Session` and `NoneType`). Import aliases, re-exports and type aliases are resolved by the checker: a declaration has the name it is declared with (`Session`, never the alias `Db`).
3. **Subtypes.** Each declaration counts with its supertypes, transitively, at most 64 (Python: the server's type hierarchy; TypeScript: the classes and interfaces in its `extends` and `implements` clauses, resolved with `textDocument/definition`). A project class deriving from `Session` is a `Session`. Assignability is nominal: a structurally compatible type that does not derive from the declaration does not count.
4. **Generics.** Type arguments are ignored: `list[Session]` is `list`, `scoped_session[Session]` is `scoped_session`.
5. **Where a declaration is.** Its file decides. Python: under `site-packages` / `dist-packages`, the package is the first path segment (`<pkg>-stubs` counts as `<pkg>`) and the module the dotted path (`sqlalchemy.orm.session`, `__init__` dropped); typeshed's `stdlib/` and `stubs/<dist>/` likewise. TypeScript: under the innermost `node_modules/`, the package is `<name>` or `@scope/name` (`@types/x` counts as `x`, `@types/a__b` as `@a/b`) and the module `<package>/<path in it without extension>` (`zod/v4/classic/schemas`); TypeScript's own `lib.*.d.ts` are package `typescript`. A declaration in the project has no package; its module is its path (Python: dotted, without a leading `src.`). The package is the one that **declares** the type, which may be a library's internal package (Vitest's `expect` is declared in `@vitest/expect`).
6. **A spec holds** when one declaration or supertype has the package (one of the list), the module (equal, or a prefix by whole segments: `.` in Python, `/` in TypeScript) and the name (one of the list) that the spec gives. With a union, one member is enough: the expression may hold such a value at run time.
7. **Unknown types.** When the checker does not know the type (Python `Any` and `Unknown`: an unannotated parameter, an unresolved import; TypeScript `any`, `unknown`, an error: no declaration), a spec does not hold, unless it says `unknown: match`. Type constraints therefore rely on typed code, which the type-checking rules enforce: `python/no-unknown` (ruff `ANN001`–`ANN003`, `ANN201`, `ANN202`: annotated parameters and returns) and `python/no-explicit-any` (`ANN401`); `typescript/no-implicit-any` (`noImplicitAny`) and `typescript/no-explicit-any`. Removing an annotation to escape a rule is then itself a finding.
8. `not` inverts its condition. A metavariable the match did not bind (an `any:` branch without it) is not evaluated.

**Where it runs.** A rule's `where` is rendered into its ast-grep rule file as `metadata: {forge_where: …}`, so each match carries it (`ast-grep scan --include-metadata`). post-edit.sh (§8.1) and check.sh step 4 (§10.1) keep the matches on added lines, then pass those with `forge_where` to `forge_types_filter` (engine.sh), which asks the language's engine and keeps the matches it confirms; other matches are untouched. An engine that cannot start or answer prints `forge:forge/types <rule-id> was not checked: <reason>`: after an edit as feedback, in check.sh as a failure.

**Schema and `forge test`.** `where` is allowed on `forbid` and `advise` targets (counted modes would need the base copy's types: not yet), with `scope.language` `python`, `typescript` or `tsx`, on metavariables that occur in the rule. Such a rule MUST have `examples.deps` (packages of its language's ecosystem, exact versions), and its examples are whole files (imports included). Its ast-grep test proves the `match` examples structurally; then every example is scanned and filtered by the generated `forge_types_filter` in a project holding those packages (a uv environment on the engine's Python, or a `bun install`ed `package.json`; kept in `$FORGE_EXAMPLES_CACHE`, default `~/.cache/forge/examples/`): a `match` example must keep a match, a `no_match` example none.

### 15.2 Engines

| Language | Server | Pin | Started as | Adapter |
|---|---|---|---|---|
| Python | `ty server` (Astral) | ty 0.0.86 | `uvx --python 3.12 --from ty==0.0.86 python <adapter> <root> <state>` | `types_python.py`: standard library only, on the Python uvx provides next to the pinned `ty` |
| TypeScript | the project's own TypeScript (`node_modules/typescript`, resolved from the root): 7 and later, its language server (`bin/tsc --lsp --stdio`, the Go compiler); 5 and 6, its `lib/tsserver.js` (tsserver protocol) | the project's (its lock file) | `bun <adapter> <root> <state>` | `types_typescript.mjs`: Node-compatible, no Bun-only API |

- **ty is Python's type checker in Forge, everywhere**: the types engine, `python/typecheck` (`ty check`, as `forge_ty` in the generated scripts) and the `tool: ty` audits run the same pinned ty through uvx, with the project's environment and configuration. basedpyright is not used.
- ty's pin follows Forge's rule for its own tools (the newest release older than 14 days; ast-grep, Squawk, zizmor, hadolint) and is embedded in the generated scripts with a revision of the adapters.
- `uvx` (Python) and `bun` (TypeScript) are required tools of the capability: missing, they are listed by `forge_missing_tools` (SessionStart, `forge status`). `forge apply` provisions the pinned ty (fills the uvx cache), so the hooks run offline afterwards. TypeScript's engine is the project's: the version that type-checks the project answers its type questions, and a project without `node_modules/typescript` gets `forge:forge/types <rule> was not checked: the project has no typescript package …`. The same semantics and the same spec tests hold for TypeScript 5, 6 and 7 (one version of each is pinned in the test tools).
- The engine answers with the project's own environment: ty finds `.venv` (or `VIRTUAL_ENV`) and the project's `[tool.ty]`; TypeScript reads the project's `tsconfig.json` files and `node_modules`, workspace packages included.
- Both adapters implement one protocol (§15.3) and the semantics of §15.1; only the way supertypes are found differs (ty has a type hierarchy; TypeScript's servers have none). The TypeScript binding `({__forge_tN: (<expr>)}).__forge_tN` declares no name, so the server keeps that version of the file until it changes: one program update per question. To tsserver only the span that differs is sent.
- **Loading.** The TypeScript adapter loads the project's tsconfig projects (one tracked source file of each `tsconfig.json`) in the background once ready, questions first; while it does, `<state>/types/typescript/loading` exists. After an edit, a question to an engine still loading waits at most `FORGE_TYPES_LOADING_WAIT` seconds (5) and then prints `forge:forge/types <rule-id> was not checked after this edit: the typescript engine (<server>) is still loading the project; the Stop check runs it`; the Stop check and CI wait for the answer. SessionStart starts the engines, so the load usually ends before the first edit.

**Evidence** (Apple M2, 2026-10-26; `typeDefinition` on a receiver; cold = spawn to `initialize` reply; first = `didOpen` + first answer; warm = `didChange` + answer, median of 20; RSS after warm-up):

| Project | Server | Cold | First | Warm | RSS |
|---|---|---|---|---|---|
| fastapi-app sample | ty 0.0.92 / basedpyright 1.40.2 | 8 / 171 ms | 42 / 478 ms | 0.5 / 11.5 ms | 43 / 354 MB |
| Netflix dispatch (655 files, SQLAlchemy 2.0.8) | ty 0.0.86 / 0.0.92 / basedpyright | 8 / 8 / 181 ms | 226 / 196 / 1338 ms | 7.5 / 6.0 / 4.3 ms | 103 / 100 / 517 MB |
| nextjs-prisma-app sample | TypeScript 7.0.2 / typescript-language-server + 5.9.3 | 45 / 65 ms | 41 / 924 ms | 1.0 / 3.5 ms | 103 / 782 MB |
| documenso (2245 TS files, workspaces) | TypeScript 7.0.2 / typescript-language-server + 5.9.3 | 42 / 63 ms | 408 / 4538 ms | 86 / 97 ms (p95 163 / 122) | 1642 / 1998 MB |

Through the TypeScript adapter on documenso (12 tsconfig projects; its own TypeScript 5.6.2, then 6.0.3 and 7.0.2 in its `node_modules`; a question on `packages/lib/utils/document-audit-logs.ts` with 2 matches of a `{package: zod}` rule; warm = after a `changed` line for an edit of that file, median of 10):

| TypeScript | Server | Ready | A question during the load | Load of the 12 projects | Warm question |
|---|---|---|---|---|---|
| 5.6.2 | tsserver | 400 ms | 9.5 s | 21.3 s | 145–152 ms (max 217) |
| 6.0.3 | tsserver | 272 ms | 7.0 s | 14.5 s | 145–153 ms (max 209) |
| 7.0.2 | tsc --lsp | 91 ms | 0.7 s | 1.2 s | 39 ms (max 71) |

tsserver's warm cost is its own program update after an edit in a large project (sending only the changed span, or one update instead of three per question, changed nothing measurable). The fixture of §15.1's tests answers 16 matches in 95–99 ms (5.9.3, 6.0.3) and 33 ms (7.0.2) cold, with the same 12 matches kept.

Through the adapters and the FIFO: a question from a POSIX `sh` client costs 5.6 ms of client and transport (bash 3.2 as `sh`); the engine answers a warm question in 2–16 ms (1 to 14 matches), the cold one in 63–250 ms; the whole `forge_types_filter` of an edit takes about 50 ms, sourcing the script included. ty and TypeScript 7 answer correctly through import aliases, re-exports, `sessionmaker()` results, subscripts, unions with `None`, subclasses (with the supertype walk) and generics, and give no declaration for `any`; queried at the end of a call or subscript, basedpyright (measured for the choice; Forge no longer uses it) and tsserver answer nothing, but through the binding of §15.1 (a name) tsserver answers every case like TypeScript 7.

### 15.3 Daemons

- **One daemon per language and repository**, in `<state>/types/<lang>/` (the hook state outside the repository, §8): `in` (the request FIFO), `started` (FIFO, start reports), `pid`, `ready` (the server's name and version), `want` (engine pin and adapter revision), `log`, `sessions/`. The adapters are `.forge/helpers/types_python.py` and `.forge/helpers/types_typescript.mjs`.
- **Start** (`forge_types_up <lang>`): SessionStart starts the engines the active rules use, in the background; any hook that needs an answer starts its engine when it is not running (the same function: there is no other path). A lock directory holding the starter's pid serialises starters; the adapter detaches into its own session (the hook that started it may end) and writes `ready …` or `failed …` to `started`; a launcher failing before it (a first download without network) gives `exited <code>`. A daemon whose `want` differs from the scripts' (another pin, another Forge) is stopped and started again. Starting takes at most `FORGE_TYPES_START_TIMEOUT` seconds (60).
- **Protocol.** Lines written to `in` are shorter than `PIPE_BUF`, so concurrent writers never interleave, and `in` is opened read-write, so a writer never blocks: `changed<TAB><path>`, `stop`, `restart`, or the absolute path of a question directory, whose `req` holds ast-grep match lines. The client opens the question's `reply` FIFO read-write before asking (the daemon's non-blocking open always finds a reader), reads up to `end` and gives up after `FORGE_TYPES_TIMEOUT` seconds (20). Questions are answered one at a time, in order; parallel hooks queue.
- **What the server sees.** The asked file is read from disk at each question and sent (`didOpen`, then `didChange` with the full text) when it differs from what the server has. Other files reach the server through `changed` lines: post-edit.sh sends every file of the edit (Edit, Write, the files a Bash command changed), turn-start.sh the uncommitted files and those a commit, checkout or pull changed since the previous turn, check.sh its change set. A change to a lock file, `pyproject.toml`, `ty.toml`, `package.json` or `tsconfig*.json` restarts the daemon instead (`restart`): the next question starts it on the new environment. Nothing is written in the repository.
- **End.** Sessions register when they start the engine or find it running; SessionEnd (`session-end.sh`) unregisters, and the last session to end stops it. A stop is synchronous (`stop<TAB><dir>`: the daemon answers once its language server has exited), so check.sh outside a session (pre-commit, CI), which stops the engines it started, leaves nothing running when it exits. A daemon also exits after `FORGE_TYPES_IDLE` seconds (1800) without a line, and within 10 seconds of its state directory or the repository disappearing, or of another daemon taking its state directory over (its `pid` file names the other one: a start whose starter was killed can race with the next one), whether lines keep coming or not. The start lock holds the starting shell's own pid (not `$$`, which a background subshell outlives), and a starter that gets the lock checks again whether the engine started meanwhile, so concurrent hooks start one engine.

### 15.4 File roles

Rules and helpers name the files they are about by **role**, not by literal globs: `scope.files: ["@python-tests"]`, `"!@alembic-migrations"`, `forge_role <role> <path>` in check helpers. Each role has Forge's defaults; `forge.yaml` changes them:

```yaml
files:
  python-tests: {add: ["integration/**/check_*.py"]}   # added to the defaults
  env-files: {set: [".env", ".env.local"]}             # replaces the defaults and the tool's config
```

- **Effective patterns** are computed by `forge apply` and rendered literally into the generated scripts (shell `case` patterns, ast-grep `files` / `ignores`), so hooks never parse configs. `forge status` prints a `Files:` section: each role an active rule uses, its patterns and where each comes from (`forge default`, the tool's config file, `forge.yaml`); a changed source is drift (`forge diff`).
- **Defaults from the tool's own config**: only when the config is data Forge reads without running code (INI, TOML, JSON, YAML, `schema.prisma`), and then they replace Forge's defaults for that role: pytest `testpaths` / `python_files` (`[tool.pytest.ini_options]`, `pytest.ini`), `alembic.ini` `script_location`, `schema.prisma` generator `output`, `package.json` `prisma.schema`, `[tool.pytest.ini_options]` `DJANGO_SETTINGS_MODULE`. Code configs (`vitest.config.*`, `playwright.config.*`, `drizzle.config.*`, `prisma.config.ts`) are never evaluated: the tool's documented defaults apply, and the user overrides in `forge.yaml`.
- **Roles and defaults:**

| Role | Defaults | From the tool's config |
|---|---|---|
| `compose` | `compose.yaml`, `compose.yml`, `docker-compose.yaml`, `docker-compose.yml`, `compose.*.yaml`, `docker-compose.*.yml` | — |
| `dockerfile` | `**/Dockerfile`, `**/Dockerfile.*`, `**/*.Dockerfile` | — |
| `python-tests` | `**/test_*.py`, `**/*_test.py`, `**/conftest.py` | pytest `testpaths` × `python_files` |
| `ts-tests` | `**/*.{test,spec}.{ts,tsx,mts,cts,js,jsx,mjs,cjs}`, `**/__tests__/**` | — (code config) |
| `e2e-tests` | `e2e/**`, `tests/e2e/**` | — (code config) |
| `alembic-migrations` | `alembic/versions/**`, `migrations/versions/**` | `alembic.ini` `script_location` + `/versions/**` |
| `prisma-migrations` | `prisma/migrations/**` | `package.json` `prisma.schema` directory + `/migrations/**` |
| `drizzle-migrations` | `drizzle/**` | — (code config) |
| `env-files` | `.env`, `.env.*`, `!@env-template` | — |
| `env-template` | `.env.example`, `.env.sample`, `.env.template` | — |
| `lockfiles` | `uv.lock`, `bun.lock`, `bun.lockb`, `package-lock.json`, `pnpm-lock.yaml`, `yarn.lock` | — |
| `docs` | `docs/**/*.{md,mdx,rst}`, `doc/**/*.{md,mdx,rst}`, `README*` | — |
| `generated-clients` | none | `schema.prisma` generator `output`; the OpenAPI generator outputs of `package.json` scripts |
| `django-settings` | `**/settings.py`, `**/settings/*.py` | pytest-django `DJANGO_SETTINGS_MODULE` |

- A role may include another (`!@env-template`); cycles are rejected. A role an active rule names but nothing matches is reported by `status`, never guessed.

### 15.5 Tests (proposed)

Test checks read the runner's own reports rather than its console output or ast-grep patterns: the tests a runner collects (`pytest --collect-only -q`, `vitest list --json`) give test identity for test integrity (§10.5) and failing tests come from its machine-readable report (pytest `--junitxml`, Vitest `--reporter=junit`) for §10.6, through engine helpers with the same interface as now. Pack rules keep naming the runner's checks; the helpers change.

### 15.6 Declared names (proposed)

`where: {F: {declared: {package: sqlalchemy, name: text}}}`: the metavariable is a name (or attribute) that refers to that declaration, through import aliases and re-exports (`textDocument/definition`, no supertypes): for functions and decorators (`text()`, `patch`, `pytest.mark.skip`, `React.useState`), where `type` would describe a function type. Same engines, daemons and semantics for package, module and name.
