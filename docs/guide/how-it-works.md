# How it works

## From rules to files

Each [rule](/guide/authoring) lists one or more **targets**: the places where it is enforced. Forge compiles every target of every active rule into the native mechanism for it.

| Target | Becomes | Runs |
|---|---|---|
| `tool` | an [audit](/guide/audits) of your Biome / ruff / basedpyright / tsc config | at `forge apply` and `forge status` |
| `ast-grep` | `.forge/ast-grep/rules/<pack>/<name>.yml` | after each edit (`forbid`), at Stop, pre-commit, CI |
| `permission` | an entry in `permissions.deny` / `permissions.ask` | by Claude Code itself |
| `guard` | a line in `guard.rules` | `PreToolUse` on every Bash command |
| `sandbox` | `sandbox.*` in `~/.claude/settings.json` | by Claude Code itself |
| `check` | a step in `.forge/check.sh` | after each edit (format, lint, typecheck), at Stop, pre-commit, CI |

A `tool` target is often followed by an `ast-grep` target: the ast-grep version is used only when the project does not use that tool. With Biome configured, `typescript/no-explicit-any` is enforced by Biome; with ESLint instead, by ast-grep.

## Generated files

**User level**

```text
~/.claude/
├── settings.json                         # appended entries only
├── skills/forge-user/
│   ├── .claude-plugin/plugin.json
│   ├── hooks/hooks.json                  # PreToolUse(Bash) guard, Notification
│   └── scripts/{guard.sh, guard.rules, notify.sh, lib.sh}
└── forge.lock.json
```

**Project level**

```text
<repo>/
├── .claude/
│   ├── settings.json                     # appended entries only
│   ├── skills/forge-project/
│   │   ├── .claude-plugin/plugin.json
│   │   ├── hooks/hooks.json
│   │   └── scripts/{lib.sh, session-start.sh, guard.sh, guard.rules,
│   │                post-edit.sh, typecheck.sh, stop.sh}
│   └── rules/forge-<pack>.md             # context facts, one per pack
└── .forge/
    ├── check.sh                          # the single check entry point
    ├── ast-grep/{sgconfig.yml, rules/, rule-tests/}
    ├── forge.lock.json
    ├── .gitignore                        # state/
    └── state/                            # not committed
```

The `skills/forge-*` folders are Claude Code plugins: Claude Code loads their `hooks/hooks.json` automatically.

## Ownership

| Category | Paths | Forge does |
|---|---|---|
| **Owned** | `.forge/**`, `.claude/skills/forge-project/**`, `.claude/rules/forge-*.md`, `~/.claude/skills/forge-user/**`, `~/.claude/forge.lock.json` | Regenerates; hashes recorded in the lock |
| **Appended** | `.claude/settings.json` (or `settings.local.json`), `~/.claude/settings.json` | Adds entries with your consent; records each; never touches entries it did not add |
| **Audited** | `biome.json`, `tsconfig.json`, `pyproject.toml`, `package.json`, dependencies | Reads and tests behavior; never writes |
| **Untouched** | `CLAUDE.md`, other `.claude/rules/*` and skills, git config, `.git/hooks/*` (except the optional pre-commit), `.husky/`, `lefthook.yml`, `.pre-commit-config.yaml`, CI files | Nothing |

## The lock

`.forge/forge.lock.json` (and `~/.claude/forge.lock.json`) records the hash of every owned file, the hash of every active rule file, and every settings entry Forge added:

```json
{
  "forge_version": "0.5.0",
  "claude_code_min": "2.1.295",
  "rules": { "sqlalchemy/no-query-api": "d73f…" },
  "files": { ".forge/check.sh": "9a1c…" },
  "settings": [{ "file": ".claude/settings.json", "path": "permissions.ask", "value": "Edit(.forge/**)" }]
}
```

It is how Forge stays honest:

- `check.sh` verifies every owned file against it first, so a file changed through Bash (which bypasses the permission prompt) fails the next Stop;
- `forge diff` and `forge status` report drifted files, settings entries that went missing, and rules whose source changed since the last apply;
- `forge remove` deletes exactly what it lists.

## Protection boundary

Permissions and the sandbox are the protection boundary; Claude Code enforces them. The [guard](/guide/checks#guard) is a tripwire for commands that cannot be expressed as a permission prefix — useful, but commands hidden behind variables or `eval` can slip past it by design.
