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
| Typecheck | basedpyright | TypeScript 7 `tsc` |
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
- Packs ship inside the package.
- Bundled libraries: YAML parser rejecting duplicate keys, anchors, aliases and custom tags; JSON Schema 2020-12 validator; JSONC reader/writer preserving comments (for `settings.json`).

### 3.1 Commands

| Command | Behavior |
|---|---|
| `forge init` | New project only. Creates the stack's tool configs from pack templates (§5.2), each only if the file does not exist, then runs `forge apply`. |
| `forge apply` | Generate or update the project-level setup in the current repository. |
| `forge apply --user` | Generate or update the user-level setup. |
| `forge apply --local` | Like `apply`, but permission entries go to `.claude/settings.local.json` (for repositories you don't own). |
| `forge diff [--user]` | Show what `apply` would change. Exit 1 if anything differs. |
| `forge status [--user] [--json]` | Active packs; rules as `Rules: <n> active, <m> enforced, <k> not enforced (<count> <reason>, …)` then each rule with where it is enforced; disabled rules with reasons; audit results with snippets; drift; missing tools. A rule is enforced when at least one of its targets acts here: a satisfied audit, an ast-grep rule, a permission, guard, sandbox or grep target, or a check whose tools are installed. Reasons: `ruff codes not selected`, `Biome rules not at error`, `basedpyright rules not at error`, `tsc options not set` (unsatisfied audits), `<tool> not configured`, `<tool> not installed`. Missing tools are what the generated scripts' `forge_missing_tools` prints (the SessionStart hook reports the same list). `--json` prints all of it as one object: `counts`, `rules` (`id`, `enforced`, `by`, `not_enforced`), `audits`, `missing_tools`, `packs`, `disabled`, `drift`. |
| `forge explain <rule-id>` | The rule file, where it is enforced in this project, and its audit result. |
| `forge test` | Validate packs and rules against the schemas; run every rule's examples (§12.1). |
| `forge remove [--user]` | Delete every owned file and exactly the settings entries recorded in the lock. |

- Every writing command accepts `--dry-run` (prints the `diff` output, writes nothing).
- `apply` is idempotent. If an owned file's hash differs from the lock, `apply` stops and prints the diff unless `--force` is given.
- Writes to a shared settings file are shown as a diff and require `y` confirmation (`--yes` for non-interactive use).

## 4. Ownership

| Category | Paths | Forge does |
|---|---|---|
| **Owned** | `.forge/**`, `.claude/skills/forge-project/**`, `.claude/rules/forge-*.md`; user level: `~/.claude/skills/forge-user/**`, `~/.claude/forge.lock.json` | Regenerates; hashes recorded in the lock |
| **Appended** | `.claude/settings.json` (or `.claude/settings.local.json` with `--local`); `~/.claude/settings.json` | Adds entries only, with consent; each entry recorded in the lock; never changes or removes entries it did not add |
| **Audited** | `biome.json`, `tsconfig.json`, `pyproject.toml`, `package.json`, dependencies | Reads and tests behavior (§11); never writes |
| **Untouched** | `CLAUDE.md`, other `.claude/rules/*`, other `.claude/skills/*`, git config, `.git/hooks/*` (except §10.3), `.husky/`, `lefthook.yml`, `.pre-commit-config.yaml`, CI files | Nothing |

`forge init` is the only command that creates audited files, and only when they don't exist.

## 5. Inputs

### 5.1 Packs

Packs form a tree: **language first, then package, then sub-package** (`typescript` → `typescript/nextjs` → `typescript/nextjs/react`). The same package can appear under several parents with different rules (`typescript/nextjs/react`, `typescript/astro/react`). User-level packs that are not tied to a language (`safety`) are roots of their own.

`packs/<path>/pack.yaml`:

```yaml
id: string                  # the pack's path in the tree: ^seg(/seg)*$, seg = ^[a-z][a-z0-9-]*$
description: string
level: user | project       # equal to the parent's level
requires: [pack-id]         # other branches only; the parent is implied
detect:                     # omitted for user-level packs
  files_any: [glob]
  deps_any: [{ecosystem: pypi | npm, name: string, version: semver-range}]
```

- A root pack is active when detected; a child pack is active when its parent is active **and** it is detected.
- `forge.yaml` `packs.add` activates a pack and its ancestors. `requires` activates the required pack and its ancestors.
- `packs.remove` deactivates a pack, its whole subtree, and every pack that requires a removed pack.
- Detection reads `pyproject.toml`, `uv.lock`, `package.json`, `bun.lock`.

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

`<repo>/.forge/rules/<name>.yaml`: project-local rules. A local rule whose `id` equals a pack rule's `id` replaces it entirely. `forge.yaml` and `.forge/rules/` are inputs, not owned output: `apply` never rewrites them, but they are behind permission `ask` (§8.2).

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
deprecated:                    # optional
  replaced_by: <rule-id>
```

An example is a string (code, written to a file inside the rule's scope) or, for rules whose targets are all `check`, a **fixture**:

```yaml
- head: {<path>: <content>}    # optional: committed first
  files: {<path>: <content>}   # then written on top: the change being checked
```

Paths are relative, without `..`. Every rule has examples, except a `check` target that runs the project's tests (stage `test`), its migrations (stage `migration`), `astro check` or a lock-file check (`uv`, `bun`): those need a whole project, and their examples are optional.

Every rule blocks. A rule you don't want is disabled with a reason.

### 6.2 Targets

| Target | Fields | Category | Effect |
|---|---|---|---|
| `tool` | `tool: biome \| ruff \| basedpyright \| tsc`, `expects` (diagnostic code or rule name), `snippet` (config to show when unsatisfied) | audited | §11 |
| `ast-grep` | `rule` (ast-grep rule object), `mode: forbid \| no-increase \| no-decrease` | owned | `.forge/ast-grep/rules/<rule-id>.yml` |
| `permission` | `decision: deny \| ask`, `rule` (permission rule string) | appended | settings `permissions` |
| `guard` | `decision: deny \| ask`, `pattern` (POSIX ERE against the JSON-escaped command) | owned | `guard.rules` (§9.2) |
| `sandbox` | `path`, `value` | appended, user only | `~/.claude/settings.json` `sandbox` |
| `check` | `stage: format \| lint \| typecheck \| migration \| test`, `run`, `changed_run` (optional, `{files}`), `fix_run` (optional, lint only, `{files}`: safe fixes applied after an edit, §8.1), `when_changed` (optional), `requires_local_db` (optional, boolean) | owned | `.forge/check.sh` (§10) |
| `grep` | `pattern` (POSIX ERE, matched line by line), `mode: forbid \| no-increase \| no-decrease`, `ignore_case` (optional) | owned | `.forge/check.sh` (§10): counts are matching lines per file |

ast-grep and grep modes:
- `forbid`: a match on an added line of a changed file fails (§10.1); with `--all`, any match.
- `no-increase`: more matches than in the base in a changed file fails.
- `no-decrease`: fewer matches than in the base in a changed file fails.

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
- **A sandbox that cannot start.** On Linux, when the setup enables the sandbox, `apply --user` and `status --user` check that bubblewrap can run it (`bwrap` and `socat` on PATH; `bwrap --ro-bind / / --dev /dev --unshare-all --die-with-parent true` succeeds). Otherwise they print why (missing packages, or bubblewrap's error: in a container with Docker's default seccomp/AppArmor profile, "Creating new namespace failed: Operation not permitted") and the ways out: give the container what bubblewrap needs, or set `"sandbox": {"enabled": false}` in `~/.claude/settings.json` knowingly (Forge never changes a value the user set).

## 8. Generated output — project level

```
<repo>/
├── .claude/
│   ├── settings.json                     # appended entries only
│   ├── skills/forge-project/
│   │   ├── .claude-plugin/plugin.json    # {"name":"forge-project", ...}
│   │   ├── hooks/hooks.json
│   │   └── scripts/{lib.sh, session-start.sh, turn-start.sh, guard.sh, guard.rules, review.sh, bash-snapshot.sh, post-edit.sh, typecheck.sh, stop.sh}
│   └── rules/forge-<pack-id>.md          # one per active pack with context.md; "/" in the id becomes "-"
└── .forge/
    ├── check.sh
    ├── ast-grep/{sgconfig.yml, rules/*.yml, rule-tests/*.yml}
    ├── forge.lock.json                   # for the CLI
    ├── manifest.sha256                   # owned-file hashes for check.sh (sha256sum format)
    ├── .gitignore                        # state/
    └── state/                            # not committed
```

### 8.1 Hooks

All scripts are POSIX `sh`. They read file paths and the working directory from the hook input (`cwd`, `tool_input.file_path`), never from `CLAUDE_PROJECT_DIR`.

| Event | Matcher | Script | Mode | Timeout | Behavior |
|---|---|---|---|---|---|
| SessionStart | `startup\|resume\|clear\|compact` | `session-start.sh` | sync | 10 s | ≤ 20 lines: branch, number of changed files and first 10 paths, last 5 commits (`%h %s`, ≤ 72 chars), active packs, missing tools (once). |
| UserPromptSubmit | — | `turn-start.sh` | sync | 10 s | Snapshots the tree at the start of the turn and resets the Stop block counter (§10.2). Prints nothing. |
| PreToolUse | `Bash` | `guard.sh` | sync | 180 s | Project `guard` rules. `onFailure: "block"`. An ask nobody can answer is reviewed (§9.3). |
| PreToolUse | `Bash` | `bash-snapshot.sh` | sync | 15 s | Snapshots the working tree for the files the command changes (§8.4). Prints nothing. |
| PermissionRequest | — | `review.sh` | sync | 180 s | §9.3. Defers to the user-level `review.sh` when it exists. |
| PostToolUse | `Edit\|Write\|Bash` | `post-edit.sh` | sync | 60 s | On the edited file (Bash: the files the command changed, §8.4): format silently; apply the `fix_run` safe fixes of in-scope lint checks, and when they changed the file add `forge: auto-fixed <codes> in <file>; re-read it before editing it again` (codes from ruff's `--show-fixes`, else "safe lint fixes"); lint it; run ast-grep `forbid` rules on it. Findings on lines added during the turn (against the turn snapshot; before the first prompt, against `HEAD`) → JSON `{"hookSpecificOutput":{"hookEventName":"PostToolUse","additionalContext":"…"}}`, ≤ 20 lines, diagnostics for these files only. Nothing on success. |
| PostToolUse | `Edit\|Write\|Bash` | `typecheck.sh` | `asyncRewake: true` | 300 s | Project typecheck, when the edited file (Bash: a file the command changed) is in a typecheck's scope. Errors in files changed during the turn (the Stop hook's change set, §10.2) → exit 2, stderr ≤ 20 lines, those files' diagnostics only; errors only in other files → silent. Skips if one is already running (lock in `.forge/state/`). |
| PostToolUseFailure | `Bash` | `post-edit.sh`, `typecheck.sh` | as above | as above | The same two hooks for a Bash command that fails or is interrupted: it may have changed files before stopping (a script that writes three files then fails on the fourth). Same checks and output, with `hookEventName` `PostToolUseFailure`. A failed Edit or Write changes nothing, so they are not matched. |
| Stop | — | `stop.sh` | sync | 600 s | §10.2. |

### 8.4 Files changed by Bash

Agents also change files through Bash (heredocs, `sed -i`, `cat >`, `git apply`), so the per-edit checks also follow Bash commands:

1. **Before** (`bash-snapshot.sh`, PreToolUse `Bash`): in a git repository with `.forge/`, write the working tree's git tree id to `.forge/state/bash/<tool_use_id>.tree`. The tree comes from `git add -A . && git write-tree` with `GIT_INDEX_FILE` set to a copy of the repository's index, `GIT_OBJECT_DIRECTORY` set to `.forge/state/bash/objects` and the repository's objects as `GIT_ALTERNATE_OBJECT_DIRECTORIES`: the user's index and object store are never written. Tracked and untracked files count; ignored ones do not. A snapshot that takes longer than `FORGE_SNAPSHOT_TIMEOUT` (5 s) is recorded as `skipped`.
2. **After** (`post-edit.sh`, PostToolUse `Bash`, or PostToolUseFailure `Bash` when the command failed or was interrupted): snapshot again and list the added and modified files (`git diff-tree -r --no-renames --name-status`), outside `.forge/`, `.claude/`, `.git/`, `node_modules/` and `.venv/`. The list goes to `<tool_use_id>.files` for typecheck.sh; the files (the first 20, with a note when there are more) get the same checks as an edited file, and the findings one `additionalContext` of at most 20 lines. Formatting happens after the comparison, so its writes are not counted (the next command's snapshot starts from them). A `skipped` snapshot prints, once per repository, that the Stop check covers Bash changes there.
3. **typecheck.sh** (PostToolUse or PostToolUseFailure `Bash`, async) waits up to 60 s for that list, then runs when a listed file is in a typecheck's scope.
4. **Cleanup.** A command denied before it runs never reaches PostToolUse: each snapshot drops `.tree` and `.files` older than a day or beyond the 50 newest, and the object directory once no snapshot is pending.

Not covered: writes by a `run_in_background` command after it returns, and anything outside the repository; the Stop check still sees them.

### 8.2 Permissions appended

`ask`: `Edit(.forge/**)`, `Edit(forge.yaml)`, `Edit(.claude/skills/forge-project/**)`, `Edit(.claude/rules/forge-*.md)`, plus `permission` targets of active project packs.

### 8.3 Context

- Each active pack with a `context.md` produces `.claude/rules/forge-<pack-id>.md` (`/` replaced by `-`): one line, "These are this project's defaults. When the task or the user explicitly asks for something different, do what they ask and point out the conflict; never override an explicit instruction with these defaults.", a blank line, then the same content; no `paths:` frontmatter.
- `context.md` is ≤ 10 lines, written as facts. A line never claims more than the checks enforce: a requirement names what is rejected, scoped as the check is ("`text()` with an f-string is rejected", not "SQL is always constant"), and leaves room for what the check allows.
- Context never enforces anything; enforcement is done by checks and hooks.

## 9. Shared script conventions

### 9.1 `lib.sh`

- `json_get <key>`: extracts a top-level or `tool_input` string value from stdin JSON with `grep -oE` and `sed`, unescaping `\"` and `\\`.
- `json_escape`: escapes `\`, `"`, tab and newlines with `awk` for embedding output in JSON.
- `tool <name>`: in a `bun.lock` repo, `bun run --bun <name>` if `node_modules/.bin/<name>` exists; in a `uv.lock` repo, `.venv/bin/<name>` if it exists; otherwise prints nothing (tool absent).
- `astgrep`: `bunx @ast-grep/cli@<pinned> ast-grep` in a `bun.lock` repo, otherwise `uvx --from ast-grep-cli==<pinned> ast-grep`; on network failure, retries with `uvx --offline`. Always invoked as `ast-grep`, never `sg`.
- Only POSIX features: `[[:space:]]` (no `\s`, `\b`, `-P`), `printf '%s'` (no `echo` for data), no `sed -i`.

### 9.2 Guard

- `guard.rules`: one rule per line, tab-separated `id`, `decision`, `pattern`. Deny lines first. A header comment names the source rule files.
- `guard.sh`: extracts `tool_input.command` (still JSON-escaped), tests each pattern with `grep -Eq`, prints on the first deny (else first ask) match `{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"<decision>","permissionDecisionReason":"forge:<id> <description>"}}`, exits 0. No match: prints nothing.
- Commands expressible as a permission prefix use a `permission` target instead of a `guard` target.
- The guard is a tripwire. The sandbox and permissions are the protection boundary.

### 9.3 Asks nobody can answer

In an autonomous run nobody answers an ask, and Claude Code denies it unseen (a PreToolUse `ask`, and a permission prompt without a `PermissionRequest` hook decision). Forge has an independent reviewer answer instead.

- **When.** `FORGE_ASK_REVIEW=always`; or, by default (`auto`), when `CLAUDE_CODE_ENTRYPOINT` is `sdk-cli`, `sdk-ts`, `sdk-py` or `claude-code-github-action` (`claude -p`, the Agent SDK, the GitHub Action; an interactive session is `cli`). Never with `FORGE_ASK_REVIEW=never`, in `dontAsk` mode (`permission_mode`), or inside a review (`FORGE_REVIEWER` set). Otherwise the scripts print nothing and Claude Code behaves as usual.
- **Guard asks.** `guard.sh` passes an `ask` to `review.sh pre <rule-id>`, and prints its `allow` or `deny` (`permissionDecisionReason`: `forge:<rule-id> reviewed by an independent session, nobody being there to answer: <reason>`). A PreToolUse `allow` does not override settings `deny` rules.
- **Permission prompts.** `review.sh` on PermissionRequest prints `{"decision":{"behavior":"allow"}}` or `{"behavior":"deny","message":"forge: denied by an independent review, nobody being there to answer: <reason>"}`. The project-level hook prints nothing when `~/.claude/skills/forge-user/scripts/review.sh` exists, so a request is reviewed once.
- **The reviewer** is `claude -p` with a prompt holding only the repository path, the tool name, its `tool_input`, and the Forge rule that asked; not the transcript or the conversation. It runs with `FORGE_REVIEWER=1`, `--tools Read,Grep,Glob --allowedTools Read,Grep,Glob --permission-mode dontAsk --setting-sources '' --settings '{"disableAllHooks":true}' --strict-mcp-config --no-session-persistence --output-format json --json-schema <decision allow|deny, reason>`, `--max-turns` 12 (`FORGE_REVIEW_MAX_TURNS`) and `FORGE_REVIEW_MODEL` as `--model` when set. Its instructions: allow only what is clearly normal work on the repository and safe unattended; deny anything irreversible, publishing or pushing, touching secrets, installing unverified packages, weakening security, tests or tool configuration, or running code from the internet; when in doubt, deny.
- **No verdict** (claude missing, no `structured_output`, a decision other than allow/deny): nothing changes; the ask stays unanswered (denied). A timeout of the guard hook blocks the call (`onFailure: "block"`).
- **Log.** One tab-separated line per review (time, session, decision, tool, input, reason) in `.forge/state/reviews.log` inside a Forge project, else `~/.claude/forge-reviews.log`.
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

**Added lines.** Outside `--all`, each changed file's added lines are computed with `git diff --no-index -U0` against its base copy; a file absent from the base counts fully. An edited line counts as added, and so does moved code. ast-grep `forbid` matches and `lint` diagnostics count only when they overlap an added line (a match spanning an edited line counts). `no-increase` / `no-decrease` rules and whole-project checks (typecheck, migrations, tests) are not line-scoped.

Runs in order, stops at the first failing step, prints `forge:<rule-id>` lines:

1. **Integrity:** every owned file under `.forge/` and `.claude/` matches its hash in `.forge/manifest.sha256`, and no other file sits in Forge's directories. check.sh never parses `forge.lock.json`, so the lock's formatting does not matter.
2. **Applied migrations:** any file under `alembic/versions/` or `prisma/migrations/` that exists in the base and is modified or deleted → fail.
3. **Tests and counts:** test files present in the base and deleted → fail; ast-grep and grep `no-increase` / `no-decrease` rules compared with the base (`forge:<id> <path>: <before> -> <after>`).
4. **Forbid rules** (ast-grep and grep), on added lines outside `--all` (`forge:<id> <path>:<line>[:<column>] <description>`).
5. **Format, lint, typecheck:** `check` targets of stages `format`, `lint`, `typecheck`, using project binaries: `changed_run` on the changed files with `--turn` and `--changed`, `run` (read-only, whole project) with `--since` and `--all`. Outside `--all`, a failing run's error diagnostics are attributed: `lint` diagnostics count on added lines, `typecheck` diagnostics (and a whole-project `format` run's) on changed files. When none counts, the check prints `forge:<rule-id> ok for the added lines (…)` or `ok for the changed files (…)` and passes; otherwise it prints only the diagnostics that count. Output naming no file at all (configuration error, crash) fails as before. Diagnostic locations are read from the tools' plain output (`path(l,c)`, `path:l:c`, `--> path:l:c`; `Would reformat: path` and Biome's `path format` name a file only; absolute paths under the repository are made relative; warning-only lines are ignored). `--all` fails on any error.
6. **Migration checks:** `check` targets of stage `migration`, only if a `when_changed` path changed and, when `requires_local_db` is set, the configured database host is `localhost`, `127.0.0.1`, `::1` or a Unix socket. Otherwise print `forge:<rule-id> skipped: no local database` and continue.
7. **Tests:** `check` targets of stage `test` (`changed_run` with `--changed`).

### 10.2 Stop hook (`stop.sh`)

`tree_hash` (in `lib.sh`) hashes `git diff HEAD` plus the untracked files (names and contents).

0. **Turn start** (`turn-start.sh`, UserPromptSubmit): write the snapshot `.forge/state/<session_id>.turn/`: `head` (the commit), `tree` (`tree_hash`), `dirty` (`<path><TAB><blob hash>` per uncommitted path, `-` when deleted) and `files/<path>` (a copy of each dirty file up to 1 MiB, for added lines); reset the block counter `.forge/state/<session_id>.blocks`. Print nothing.
1. With a snapshot: if `tree_hash` and `HEAD` equal the snapshot's, the tree did not change during the turn: exit 0. The change is then what differs from the snapshot, commits made during the turn included; the user's own uncommitted work from before the turn is not part of it. Without a snapshot (no prompt seen yet, e.g. the first Stop after `forge apply`), the change is every uncommitted file (`--changed`).
2. If the tree (and, with a snapshot, `HEAD`) equals the last passing one (`<snapshot>/pass`, else `.forge/state/last-pass`), exit 0.
3. If the block counter is ≥ 3: reset it, print `{"systemMessage":"forge: check failed 3 times. Handing over to you."}`, exit 0.
4. Run `check.sh --turn <snapshot>` (or `--changed`).
5. Pass: record the tree as passing, reset the counter, print nothing.
6. Fail: increment the counter, print `{"decision":"block","reason":"<check output, ≤ 20 lines>"}`.

### 10.3 Pre-commit and CI

- `forge apply` offers to create `.git/hooks/pre-commit` (running `sh .forge/check.sh --changed`) only if no hook manager is present (`.husky/`, `lefthook.yml`, `.pre-commit-config.yaml`, `core.hooksPath` set) and the file does not exist. Otherwise `forge status` prints the line to add to the existing manager.
- `forge status` prints the CI steps: `sh .forge/check.sh --since origin/<default branch>` for pull requests (the checkout needs the base branch's history: `fetch-depth: 0`), and `sh .forge/check.sh --all` for the default branch. The default branch is `origin/HEAD`, else `main`.

### 10.4 Config ratchet

`python/config-ratchet` and `typescript/config-ratchet` are `check` targets (stage `lint`) running the engine helper `forge_ratchet <runtime> <script>`. The script is embedded in `check.sh` as a here-document, only when the rule is active (`ratchet.py`, run with the project's own `.venv/bin/python`, 3.11+ for `tomllib`; `ratchet.mjs`, run with `bun`); a separate file would be rewritten by the project's own formatters. It compares each changed config file with its copy in the base (§10.1) and prints one line per loosened setting, `<path>: <setting> <before> -> <after>` (or `adds <x>`, `no longer has <x>`). Only the relevant sections are parsed, so dependency changes never trigger it:

| File | Loosened when |
|---|---|
| `pyproject.toml` `[tool.ruff]`, `ruff.toml`, `.ruff.toml` | a selected code is no longer covered by `select` / `extend-select` (prefixes and `ALL` count); a code added to `ignore` / `extend-ignore` or to a glob of `per-file-ignores`; a path added to `exclude` / `extend-exclude` |
| `[tool.basedpyright]`, `[tool.pyright]`, `pyrightconfig.json` | `typeCheckingMode` lowered (`off` < `basic` < `standard` < `strict` < `recommended` < `all`; removed = the tool's default); a `report*` level lowered or removed; `failOnWarnings` no longer true; `enableTypeIgnoreComments` no longer false; `exclude` / `ignore` entries added; `strict` paths removed |
| `[tool.pytest.ini_options]` | `addopts` gains `--ignore`, `--ignore-glob`, `--deselect`, `-k`, `-m`, `--lf`, `--sw` or `--continue-on-collection-errors`; `testpaths` entries removed; `norecursedirs` entries added; `xfail_strict` no longer true |
| `[tool.coverage]` | `report.fail_under` lowered or removed; `omit`, `exclude_lines` or `exclude_also` entries added |
| `biome.json`, `biome.jsonc` | a rule's level lowered (`error` > `warn` > `info` > `off`) or removed; a group's level lowered; `recommended`, `linter.enabled` or `formatter.enabled` turned off; negated `files.includes` / `linter.includes` entries added; an `overrides` entry added or changed |
| `tsconfig*.json` | a strict flag (`strict`, `noImplicitAny`, `strictNullChecks`, … `noUncheckedIndexedAccess`, `exactOptionalPropertyTypes`, `noUnused*`, `checkJs`) no longer true; `skipLibCheck` or `ignoreDeprecations` added; `exclude` entries added; `include` entries removed; `extends` changed |
| `package.json` `scripts` | a script named like `test`, `check`, `lint`, `type…`, `e2e`, `ci`, `verify` removed, or given `--passWithNoTests`, `\|\| true`, `--no-errors-on-unmatched`, an exclude/ignore/skip flag, `--max-warnings N`, `--silent` |

A finding whose exact line appears in `forge.yaml` (an `accept` entry) is not reported. Otherwise the check fails with the findings and how to accept one. When the runtime is missing the check is `skipped: <tool> not installed`; when the script cannot run (no `tomllib`, a crash) it prints `forge:<id> skipped: …` and does not block. JS and TS config files (`vitest.config.*`, `playwright.config.*`) are code and are not parsed: they are behind permission `ask` only. The ratchet applies to everyone: a human loosening a config is stopped at pre-commit and in CI too, until the change is accepted in `forge.yaml`.

### 10.5 Test integrity

The engine helper `forge_test_edits` acts only at Stop (`--turn`): Claude may add tests, including in existing test files, but may not change or remove a test that existed at the start of the turn. For each in-scope file present in the base, it lists the tests at the start of the turn and now with ast-grep (`--inline-rules`, the pinned version): pytest `test*` functions and methods with their decorators, keyed by enclosing classes and name; Vitest and Playwright `test(…)` / `it(…)` calls (any modifier: `.only`, `.skip`, `.each(…)`, …; not `test.describe`, `test.step` or hooks), keyed by the enclosing `describe` / `test.describe` names and the test name (`#2` for a repeated name). A test whose text differs, whitespace aside, prints `<path>: test "<key>" was changed`; one that is gone prints `… was removed`. The check then fails with: "Tests that existed before this turn are not edited to make the code pass. Restore them; if one looks wrong, stop and ask the user." Pre-commit and CI do not run it: a human edits tests on purpose. It complements `no-fewer-asserts`, `no-fewer-expects` and `no-new-skip`, which miss a changed expected value.

## 11. Audit of tool targets

For each `tool` target of an active rule:

1. The tool is not present in the project → the rule's `ast-grep` target is used if it has one; otherwise `forge status` reports "not enforced here: <tool> not configured".
2. The tool is present → Forge writes each example to a temporary file inside the rule's scope and runs the project's own tool with the project's config, reading machine-readable output: `biome lint --reporter=json <file>`, `ruff check --output-format json --stdin-filename`, `basedpyright --outputjson <file>`, `tsc --noEmit -p <temporary tsconfig extending the project's>`.
3. Every `match` example gets the expected diagnostic **at a level that fails the tool's run**, and no `no_match` example gets it → satisfied (stricter configs satisfy automatically). A diagnostic fails the run when its severity is `error`, or when it is a warning and the run exits non-zero without any error diagnostic (the tool fails on warnings, e.g. basedpyright's `failOnWarnings`). ruff has no warning level; tsc reports errors only.
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
- **Owned-file hashes** are sha256 of the content, except for `.json` files, which are hashed with spaces, tabs, CR and LF removed: a formatter reflowing Forge's JSON is not drift, any other change is. A file whose owned hash matches is not rewritten by `apply`. The lock is compared as JSON (formatting ignored).
- **`.forge/manifest.sha256`** (project level) repeats the owned-file hashes of `.forge/` and `.claude/` as `<sha256>  <path>` lines (sha256sum format) for `check.sh`. Non-JSON lines verify with `sha256sum -c`; JSON lines need the whitespace-stripped content, so `check.sh` hashes each file itself (`sha256sum`, `shasum -a 256` or `openssl`). The manifest and the lock do not hash themselves; `remove` deletes both.
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
| `safety/prisma-ai-consent` | `guard` deny: any command containing `PRISMA_USER_CONSENT_FOR_DANGEROUS_AI_ACTION` (only the user gives that consent) |
| `safety/terraform-destroy` | `guard` ask: `terraform` / `tofu` `destroy`, `apply … -auto-approve` |
| `safety/git-stash-drop` | `guard` ask: `git stash drop` (`git stash clear` is denied by `git-history-rewrite`) |
| `safety/git-branch-force-delete` | `guard` ask: `git branch -D`, `--delete --force`, `-f -d` |
| `safety/ephemeral-exec` | `guard` ask: packages fetched from a registry on the fly: `pnpm dlx`, `yarn dlx`, `pipx run`, `npm exec`, `uvx`, `uv tool run`; `npx` / `bunx` / `bun x` with `-y`, `--yes`, `-p`, `--package` or a `name@version` spec. `bunx prisma` (a tool installed in the project) is not asked |
| `safety/docker-privileged` | `guard` deny: `docker run` / `create` / `exec` with `--privileged`, `--pid=host`, `--ipc=host`, `--userns=host`, `--cap-add ALL` or `SYS_ADMIN`, `--security-opt seccomp=unconfined` or `apparmor=unconfined`, `docker.sock`; `run` / `create` mounting `/`, `/etc`, `/root`, `~` or `$HOME` (`-v`, `--volume`, `--mount source=`) |
| `safety/docker-push` | `guard` ask: `docker push`, `docker image push`, `docker compose push`, `docker buildx … --push` |

### python

| Rule | Targets |
|---|---|
| `python/format` | `check` format: `run: ruff format --check .`, `changed_run: ruff format {files}` |
| `python/lint` | `check` lint: `run: ruff check .`, `changed_run: ruff check {files}`, `fix_run: ruff check --fix --fixable I,UP --exit-zero --show-fixes {files}` (sorted imports and pyupgrade, when the project selects them; never F401 / F841) |
| `python/typecheck` | `check` typecheck: `run: basedpyright` |
| `python/no-explicit-any` | `tool` basedpyright `reportExplicitAny`; `tool` ruff `ANN401` |
| `python/no-bare-except` | `tool` ruff `E722` |
| `python/no-unscoped-ignore` | `tool` basedpyright `reportIgnoreCommentWithoutRule`; `tool` ruff `PGH003` |
| `python/no-unknown` | `tool` basedpyright `reportUnknownParameterType`, `reportUnknownVariableType` |
| `python/no-useless-cast` | `tool` basedpyright `reportUnnecessaryCast`, `reportInvalidCast` |
| `python/no-deprecated` | `tool` basedpyright `reportDeprecated` (PEP 702 and typeshed deprecations) |
| `python/no-blocking-in-async` | `tool` ruff `ASYNC210`, `ASYNC230`, `ASYNC251` |
| `python/no-tls-bypass` | `tool` ruff `S501` |
| `python/no-shell-true` | `tool` ruff `S602` |
| `python/no-stale-ignore` | `tool` basedpyright `reportUnnecessaryTypeIgnoreComment` |
| `python/aware-datetime` | `tool` ruff `DTZ005` |
| `python/no-new-suppressions` | `grep` no-increase: `# noqa`, `# type: ignore`, `# pyright: ignore` (suppression budget) |
| `python/config-files` | `permission` ask: `Edit(**/ruff.toml)`, `Edit(**/.ruff.toml)`, `Edit(**/pyrightconfig.json)` |
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
| `python/no-placeholder-comment` | `grep` forbid (case-insensitive): comments such as `TODO: implement`, "in a real implementation", "placeholder logic", "stub for now" |
| `python/env-example` | `check` lint: `forge_env_example py`, not with `--all`: a field added to a pydantic-settings `BaseSettings` class (`env_prefix`, `alias`, `case_sensitive` applied) appears in the nearest directory's `.env.example`, `.env.template`, `.env.sample`, `.env.dist` or `.env.<name>.example` (a `# KEY=` line counts): always without a default, also with one when the template lists at least half of the class's variables; a template listing none of them is not about that class |

context.md: uv and `.venv/bin`, basedpyright; no blocking calls in `async def`; ignore comments name their rule.

### typescript

| Rule | Targets |
|---|---|
| `typescript/format-lint` | `check` lint: `run: biome ci .`, `changed_run: biome check --write {files}`, `fix_run: biome check --write --formatter-enabled=false --skip=correctness/noUnusedImports --skip=correctness/noUnusedVariables {files}` (safe lint fixes and import sorting) |
| `typescript/typecheck` | `check` typecheck: `run: tsc --noEmit -p tsconfig.json` |
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
| `typescript/env-example` | `check` lint: `forge_env_example ts`, as `python/env-example` for the UPPER_CASE keys of a Zod env schema (t3-env `createEnv`, or `z.object(…)` with `process.env`) whose value is a `z.` chain; a key with `.default()`, `.optional()` or `.catch()` counts as having a default |

context.md: strict TypeScript and Bun; TypeScript 6 deprecations are migrated, never silenced with `ignoreDeprecations`; no `!` or `as unknown as`.

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
| `python/sqlalchemy/fk-index` | `ast-grep` forbid: `mapped_column(…)` or `Column(…)` with a `ForeignKey(…)` argument and no `index=True`, `unique=True` or `primary_key=True`, unless its class or `Table(…)` declares an `Index`, `UniqueConstraint` or `PrimaryKeyConstraint` |

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
| `python/sqlalchemy/alembic/fk-index` | `check` lint on `alembic/versions/**`: `forge_alembic_fk_index`: in `upgrade()`, each column given a foreign key (`sa.Column(…, sa.ForeignKey(…))`, `sa.ForeignKeyConstraint([…])`, `op.create_foreign_key(…, […], …)`) leads an `op.create_index`, or has `index=True`, a primary key, a unique constraint or a `CREATE INDEX`; skipped for a MySQL database URL |
| `python/sqlalchemy/alembic/sqlite-batch` | `check` lint on `alembic/versions/**`: `forge_alembic_batch`: when `alembic/env.py` sets `render_as_batch=True` or `alembic.ini` has a sqlite URL, no `op.alter_column`, `op.drop_column`, `op.create_foreign_key`, `op.drop_constraint`, `op.create_unique_constraint`, `op.create_check_constraint`, `op.create_primary_key` outside `op.batch_alter_table()`, except under an `if` on the dialect |
| `python/sqlalchemy/alembic/data-migration-downgrade` | `check` lint on `alembic/versions/**`: `forge_alembic_data_downgrade`: an `upgrade()` that changes rows (`UPDATE … SET`, `INSERT INTO`, `DELETE FROM` with `execute(`, `sa.update/insert/delete(`, `.update()`, `op.bulk_insert`, `Session(`) has a `downgrade()` that is more than `pass`, `...` and a docstring, unless a test file mentions Alembic (`pytest_alembic`, `alembic.command`, …) |

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
| `python/pytest/run` | `check` test: `run: pytest -q`, `changed_run`: changed test files plus `tests/**/test_<module>.py` for each changed module; `pytest --testmon` when the project depends on pytest-testmon; timeout 300 s |
| `python/pytest/no-new-skip` | `ast-grep` no-increase on test files: `@pytest.mark.skip($$$)`, `@pytest.mark.skipif($$$)`, `@pytest.mark.xfail($$$)`, `pytest.skip($$$)`, `@unittest.skip($$$)`, `pytest.importorskip($$$)` |
| `python/pytest/no-fewer-asserts` | `ast-grep` no-decrease on test files: `assert $$$` |
| `python/pytest/no-test-env-branch` | `ast-grep` forbid on non-test files: `PYTEST_CURRENT_TEST` lookups, `"pytest" in sys.modules` |
| `python/pytest/no-blind-raises` | `tool` ruff `B017` |
| `python/pytest/no-tautological-assert` | `ast-grep` forbid on test files: `assert` on a literal (`True`, a number, a string), `assert $X == $X`, `assert $X is $X` |
| `python/pytest/no-mock-increase` | `ast-grep` no-increase on test files: calls to `patch`, `patch.object`, `patch.dict`, `MagicMock`, `AsyncMock`, `Mock` (bare, `mock.`, `unittest.mock.` or `mocker.`) |
| `python/pytest/no-test-edits` | `check` lint: `forge_test_edits` on `**/test_*.py`, `**/*_test.py` (§10.5) |

context.md: tests assert behaviour, never weakened; "If an existing test looks wrong, stop and ask; never edit it to make the code pass."

### typescript/bun

| Rule | Targets |
|---|---|
| `typescript/bun/lock-sync` | `check` lint: `run: bun install --frozen-lockfile`, `when_changed: [package.json]` |
| `typescript/bun/no-other-pm` | `guard` ask: `npm install`, `npm i`, `npm ci`, `pnpm`, `yarn` |
| `typescript/bun/release-age` | `check` lint: `forge_bun_cooldown`: `bunfig.toml` `[install] minimumReleaseAge` ≥ 259200; `when_changed`: `bunfig.toml`, `package.json`, `bun.lock` |
| `typescript/bun/trusted-deps` | `check` lint: `forge_trusted_deps`: no `trustedDependencies` entry added to a `package.json` since the base (with `bun`) |

### typescript/nextjs

| Rule | Targets |
|---|---|
| `typescript/nextjs/no-pages-data` | `ast-grep` forbid in `app/**`, `src/app/**`: `getServerSideProps`, `getStaticProps` exports |
| `typescript/nextjs/no-next-router` | `ast-grep` forbid: `import $$$ from "next/router"` |
| `typescript/nextjs/proxy-not-middleware` | `ast-grep` forbid: any code in `middleware.ts` / `src/middleware.ts` (file renamed to `proxy.ts` in Next.js 16) |
| `typescript/nextjs/revalidate-tag-profile` | `ast-grep` forbid: `revalidateTag($A)` with one argument |
| `typescript/nextjs/no-legacy-image-config` | `ast-grep` forbid: imports of `next/legacy/image`; a `domains` key inside `images` |
| `typescript/nextjs/server-action-validate` | `ast-grep` forbid: an exported async function with parameters in a `'use server'` file, or an exported `POST` / `PUT` / `PATCH` / `DELETE` function, without a `.parse` / `.safeParse` / `.parseAsync` / `.safeParseAsync` call |
| `typescript/nextjs/server-action-auth` | `ast-grep` forbid: the same functions without a call to `auth`, `getSession`, `getServerSession`, `getCurrentUser`, `currentUser`, `getUser`, `requireUser`, `requireAuth`, `requireSession`, `verifySession`, `validateRequest` or `getAuth` (as a function or a method). Another helper name, or a public endpoint, is handled with a local rule of the same id (§5.3) |
| `typescript/nextjs/no-public-secret-env` | `ast-grep` forbid: `process.env.NEXT_PUBLIC_*` names containing `SECRET`, `PRIVATE`, `PASSWORD`, `SERVICE_ROLE` or `DATABASE_URL` |

context.md: Next.js 16 App Router: async request APIs, Server Components by default, `proxy.ts`, `revalidateTag(tag, profile)` / `updateTag(tag)`, `images.remotePatterns`, no `next lint`, Server Actions validate input and check authorization, no secrets in `NEXT_PUBLIC_`.

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
| `typescript/prisma/migration-present` | `check` migration: `run: prisma migrate diff --from-migrations prisma/migrations --to-schema prisma/schema.prisma --exit-code`, `when_changed: [prisma/schema.prisma]`, `requires_local_db: true`; skipped with a message if `prisma.config.ts` has no `shadowDatabaseUrl` |
| `typescript/prisma/no-reset` | `guard` deny: `prisma migrate reset`, `prisma db push --force-reset`, `prisma db push --accept-data-loss` |
| `typescript/prisma/db-push` | `guard` ask: `prisma db push`, `prisma migrate deploy`, `prisma db execute` |
| `typescript/prisma/generated-client-import` | `ast-grep` forbid: `import $$$ from "@prisma/client"` |
| `typescript/prisma/no-middleware` | `ast-grep` forbid: `$C.$use($$$)` |
| `typescript/prisma/no-raw-unsafe` | `ast-grep` forbid: `$C.$queryRawUnsafe($$$)`, `$C.$executeRawUnsafe($$$)` |
| `typescript/prisma/client-generator` | `check` lint: no `provider = "prisma-client-js"` in tracked `*.prisma` files, `when_changed: [**/*.prisma]` |
| `typescript/prisma/migration-lint` | `check` migration: changed `prisma/migrations/*/migration.sql` linted with Squawk (§13.1); `--all`: the newest migration; `when_changed: [prisma/migrations/**]` |

context.md: Prisma 7: `prisma-client` generator with `output`, driver adapters, `prisma.config.ts` (no automatic `.env`), `prisma db seed`, `$extends` (no `$use`), tagged `$queryRaw` only.

### typescript/vitest

| Rule | Targets |
|---|---|
| `typescript/vitest/run` | `check` test: `run: vitest run`, `changed_run: vitest run --changed` |
| `typescript/vitest/no-new-skip` | `ast-grep` no-increase on test files: `it.skip`, `test.skip`, `describe.skip`, `it.todo`, `test.todo`, `describe.todo`, `$X.skipIf`, `$X.runIf`, `$X.only` |
| `typescript/vitest/no-fewer-expects` | `ast-grep` no-decrease on test files: `expect($$$)` |
| `typescript/vitest/no-test-env-branch` | `ast-grep` forbid on non-test files: `process.env.VITEST`, `process.env.NODE_ENV === "test"` |
| `typescript/vitest/no-tautological-expect` | `ast-grep` forbid on test files: `expect(<literal>).$M(…)` (`true`, `false`, `null`, `undefined`, a number or a string) |
| `typescript/vitest/no-snapshot-update` | `guard` ask: `vitest` with `-u` or `--update` |
| `typescript/vitest/no-workspace-file` | `check` lint: no tracked `vitest.workspace.*` (Vitest 4 ignores it), `when_changed: [vitest.*]` |
| `typescript/vitest/no-pass-with-no-tests` | `guard` ask: `vitest … --passWithNoTests` |
| `typescript/vitest/config-file` | `permission` ask: `Edit(**/vitest.config.*)` |

| `typescript/vitest/no-mock-increase` | `ast-grep` no-increase on test files: `vi.mock`, `vi.doMock`, `vi.spyOn`, `vi.fn` calls |
| `typescript/vitest/no-test-edits` | `check` lint: `forge_test_edits` on `**/*.test.*`, `**/*.spec.*`, `**/__tests__/**`, not `e2e/**` (§10.5) |

context.md: Vitest 4 `test.projects` and `maxWorkers`; assertions are never weakened to make a test pass; "If an existing test looks wrong, stop and ask; never edit it to make the code pass."

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
| `typescript/playwright/no-test-edits` | `check` lint: `forge_test_edits` on `e2e/**` (§10.5) |

context.md: locators (`getByRole`, `getByLabel`, `getByText`, `getByTestId`) and web-first assertions; never `waitForTimeout`; `@axe-core/playwright` for accessibility scans; "If an existing test looks wrong, stop and ask; never edit it to make the code pass."

### typescript/zod

| Rule | Targets |
|---|---|
| `typescript/zod/v4-string-formats` | `ast-grep` forbid: a string-format method (`email`, `uuid`, `url`, `datetime`, …) called on a chain rooted at `z.string()` |
| `typescript/zod/error-param` | `ast-grep` forbid: a `message`, `required_error`, `invalid_type_error` or `errorMap` key in an object passed to a call on a `z.` chain |

context.md: Zod at boundaries with `z.infer`; Zod 4 top-level formats, the single `error` parameter, `z.strictObject` / `z.looseObject`, `error.issues`, `z.treeifyError`.

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

## 14. Acceptance tests

The test suite in `tests/spec/` is the executable form of this specification: one file per section (`@spec <number> · <title>` header), one requirement per test title. The documentation site renders those files with the JUnit results of the CI run (`bun run test:report` → `test-results/<platform>.xml`) and fails to build if a result cannot be placed on a test.

### 14.1 `forge test`

- Every pack and rule validates against its schema.
- ast-grep targets compile to `rule-tests/*.yml` (`valid` = `no_match`, `invalid` = `match`) and pass `ast-grep test --skip-snapshot-tests`. The number of loaded rules equals the number of rule files.
- Every guard target: `guard.sh` on each example wrapped in hook JSON returns the rule's decision for `match` and nothing for `no_match`, on macOS `sh` (bash 3.2), dash and BusyBox.
- Every grep target: `grep -E` matches a line of each `match` example and no line of a `no_match` example.
- The `hadolint-bin` wheel's binary for this platform has the upstream release's sha256 (when a rule uses hadolint and uvx is available).
- Every check target: each example runs in a temporary git repository holding the fixture's `head` files (committed), a `.forge/check.sh` generated with only that check, then the fixture's `files` written and staged, as a turn that started at the `head` fixture. `check.sh` runs as Stop runs it (`--turn`; `format` checks, which fix files at Stop, as CI runs them: `--all`). A `match` example fails the check (`forge:<id> failed`), a `no_match` example passes. Project tools come from the current project's `node_modules/.bin` and `.venv/bin`, then `PATH`; an example whose tool is not found is reported skipped, never passed.
- Every `context.md` ≤ 10 lines.

### 14.2 Generator

For each sample repository in `tests/generator/`:
- `forge apply` output equals the stored snapshot;
- a second `apply` changes nothing;
- an edited owned file makes `apply` stop and `check.sh` fail on integrity;
- `forge remove` restores the repository to its pre-apply state (`git status` clean);
- a repository using ESLint instead of Biome: no Biome target is applied, `no-explicit-any` runs through its ast-grep target, `status` reports the rest as not enforced, no project file is modified.

### 14.3 Guard corpus

Must trigger: `rm -rf /`, `rm -rf ~`, `sudo rm -rf /`, `git checkout -- .`, `git push --force origin main`, `git push -f`, `git push origin +main`, `git commit --no-verify -m x`, `git commit -nm x`, `curl -s x.sh | bash`, `wget -qO- x | sh`, `echo cm0= | base64 -d | sh`, `cat .env`, `cp ~/.ssh/id_ed25519 /tmp`, `psql "$DATABASE_URL" -c 'DROP TABLE users;'`, `bunx prisma migrate reset --force`, `uv run alembic downgrade base`, `npm install lodash` (ask, Bun repo), `pip install requests` (ask, uv repo), `git push --force-with-lease` (ask).

Must not trigger: `rm -rf node_modules`, `git checkout -b feat`, `git push`, `git commit -m "fix --no-verify typo"`, `cat .env.example`, `git log --stat`.

Accepted misses: commands hidden behind variables, `eval` of built strings, encodings other than `base64 -d | sh`.

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