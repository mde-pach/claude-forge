## 12. Lock

`forge.lock` is TOML, at the repository root, written by `forge apply` and committed. The user level's lock is `~/.claude/forge-user.lock`, with the same format. Its first line is `# Generated file: do not edit.`. The agent is refused reading it (§9.3): nothing of its work needs what Forge recorded.

```toml
# Generated file: do not edit.
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

[[settings]]                       # each settings entry apply added to a committed settings file
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
- `[[settings]]` records the entries of `.claude/settings.json` and `.mcp.json` (project level) or of `~/.claude/settings.json` (user level). The entries apply adds to `.claude/settings.local.json` (the sandbox entries of §7.2, and every entry with `--local`) are this machine's and their values may be too (a Docker socket's path): recording them in the committed lock would make it differ between teammates. They are recorded in this machine's state instead (below).
- `[[settings]]`, the local record and `[[blocks]]` are exactly what `remove` takes out.
- Unknown keys are refused. Every command that reads the lock validates it and refuses it as a whole when it names a path Forge does not write (§4.2): `forge.lock: refused: …`. A lock that does not parse: `forge.lock: <error>; fix it or delete it`.
- A lock written by an earlier Forge may record entries of `.claude/settings.local.json`: they are taken as this machine's, and the next `apply` writes the lock without them and records them locally.
- Hashes are lowercase hex sha256 of the exact bytes.

### The local record

`<state>/<repo-id>/local-settings.json` (§4.3) records the entries `apply` added to `.claude/settings.local.json`, in the order the plan wants them:

```json
{
  "settings": [
    {"file": ".claude/settings.local.json", "path": "sandbox.network.allowUnixSockets", "value": "~/.orbstack/run/docker.sock"}
  ]
}
```

- `apply` writes it before the settings files (a recorded entry that a failed write left out is harmless to remove), only when it changed; none is created while it would be empty, so a missing record means no entry was added on this machine, or the state was wiped. It is not a change to the project: `forge diff` does not show it and `apply` can be `up to date` after writing it.
- `apply`, `diff` and `status` take its entries as the lock's: one that went missing is drift (`<file>: missing <path> <value>`) and `apply` re-adds it; one the plan no longer wants is taken out of the file.
- `remove` takes out the lock's entries and the record's, then deletes the record. With no record, it cannot know which entries of `.claude/settings.local.json` are Forge's, so it leaves the file (never deleting an entry the person wrote) and says so when the file exists: `forge: note: this machine has no record of the entries forge added to .claude/settings.local.json (<record path> is missing), so that file is left as it is; check it and remove forge's entries yourself`.
- Its entries must name `.claude/settings.local.json`; a record that does not parse or names another file is refused: `forge: <record path>: <error>; fix it or delete it (…)`.

### Tests

- Round trip, and the launcher agreeing with the full parser: `crates/forge-render/src/lock.rs::round_trip_and_launcher_agrees`, `::paths` (unit); `crates/forge-launcher/src/lock.rs` (unit); `crates/forge-launcher/tests/lock_differential.rs`.
- The lock each sample gets, drift against `[files]`, `remove` using `[[settings]]` and `[[blocks]]`: `crates/forge-cli/tests/apply.rs` (snapshots in `crates/forge-cli/tests/snapshots/`).
- The local record: its format, `crates/forge-render/src/lock.rs::local_record_round_trip` (unit); apply records the sandbox entries there and not in the lock, a second apply is up to date, remove takes them out: `crates/forge-cli/tests/apply.rs::testcontainers_sandbox_entries_go_to_the_local_settings`; `--local`, drift and re-adding, remove keeping the person's entry: `::local_entries_are_recorded_on_this_machine_not_in_the_lock`; a lost record: `::remove_without_the_local_record_leaves_the_local_entries`; an earlier lock's local entries: `::local_entries_in_an_earlier_lock_move_to_the_record`.
- Refused locks (not TOML, an unknown key, a path Forge does not write): `crates/forge-cli/tests/spec_cli.rs::invalid_locks_and_settings_are_refused`.
