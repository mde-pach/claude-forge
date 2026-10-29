## 12. Lock

`forge.lock` is TOML, at the repository root, written by `forge apply` and committed. The user level's lock is `~/.claude/forge-user.lock`, with the same format.

```toml
# Written by forge apply. Do not edit.
forge = "0.6.0"                    # the Forge version that wrote it; the launcher runs this version (§3.4)
claude_code_min = "2.1.295"        # the Claude Code minimum of that version (§2.3)
rule_set = "<sha256>"              # the active rule set (below)

[rules]                            # each active rule id → sha256 of its rule file
"python/no-eval" = "<sha256>"

[files]                            # each owned file apply wrote → sha256 of its content
".claude/rules/forge-python.md" = "<sha256>"

[roles]                            # each file role the active rules use → its effective patterns
alembic-migrations = ["alembic/versions/**"]

[tools]                            # the tool pins of this Forge version (§17.2)
uv = "0.13.1"
ty = "0.0.86"
python = "3.12"

[[settings]]                       # each settings entry apply added
file = ".claude/settings.json"
path = "permissions.ask"
value = "Edit(forge.yaml)"         # a string value; any other value is `json = "<JSON text>"`

[[blocks]]                         # Forge's block in a hook manager's file (§7.6)
file = ".pre-commit-config.yaml"
kind = "pre-commit"                # pre-commit | husky | lefthook | hooks
created = false                    # whether Forge created the file
hash = "<sha256>"                  # of the block's lines, indentation removed
```

- `forge` is the first key, a top-level string holding an exact version (`MAJOR.MINOR.PATCH[-pre][+build]`). The launcher reads only this key, with a scanner that stops at the first table header; a differential test checks it agrees with a full TOML parser.
- `rule_set` is the sha256 of the active rules (id and file hash), the file roles they use (effective patterns) and the active packs. The hooks and the checker key their caches on it.
- `[rules]` lets `apply` and `status` report `rule <id> changed since the last apply`.
- `[files]` is what drift is measured against: an owned file whose content no longer has its recorded hash is drift (§3.3). It lists the rules files and, when Forge created it, git's pre-commit hook.
- `[roles]` lets `status` report a file role whose patterns changed since the last apply.
- `[tools]` holds this Forge version's tool pins, written by every `apply`. `forge status` lists each pin that differs between the lock and the running Forge (`<tool>: <locked> -> <ours>`; `not pinned` for a missing side).
- `[[settings]]` and `[[blocks]]` are exactly what `remove` takes out.
- Unknown keys are refused. Every command that reads the lock validates it and refuses it as a whole when it names a path Forge does not write (§4.2): `forge.lock: refused: …`. A lock that does not parse: `forge.lock: <error>; fix it or delete it`.
- Hashes are lowercase hex sha256 of the exact bytes.

### Tests

- Round trip, and the launcher agreeing with the full parser: `crates/forge-render/src/lock.rs::round_trip_and_launcher_agrees`, `::paths` (unit); `crates/forge-launcher/src/lock.rs` (unit); `crates/forge-launcher/tests/lock_differential.rs`.
- The lock each sample gets, drift against `[files]`, `remove` using `[[settings]]` and `[[blocks]]`: `crates/forge-cli/tests/apply.rs` (snapshots in `crates/forge-cli/tests/snapshots/`).
- Refused locks (not TOML, an unknown key, a path Forge does not write): `crates/forge-cli/tests/spec_cli.rs::invalid_locks_and_settings_are_refused`.
