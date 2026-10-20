# CLI

```sh
bunx @maximedepachtere/forge <command> [options]
npx  @maximedepachtere/forge <command> [options]
```

| Command | Behavior |
|---|---|
| `forge init` | New project only: creates the stack's tool configs from pack templates (each only if missing), then runs `forge apply`. |
| `forge apply` | Generates or updates the project-level setup in the current git repository. |
| `forge apply --user` | Generates or updates the user-level setup in `~/.claude/`. |
| `forge apply --local` | Like `apply`, but permission entries go to `.claude/settings.local.json` (repositories you don't own). |
| `forge diff [--user]` | Shows what `apply` would change. Exit 1 if anything differs. |
| `forge status [--user]` | Active packs and why, active rules and where each is enforced, disabled rules with reasons, audit results with snippets, drift, missing tools, the pre-commit and CI lines. |
| `forge explain <rule-id>` | The rule file, where it is enforced in this project, and its audit. |
| `forge test` | Validates packs and rules against the schemas and runs every rule's examples. |
| `forge remove [--user]` | Deletes every owned file and exactly the settings entries recorded in the lock. |

## Options

| Option | Applies to | Effect |
|---|---|---|
| `--dry-run` | `init`, `apply`, `remove` | Prints the diff; writes nothing. |
| `--yes`, `-y` | writing commands | Confirms settings changes and the pre-commit hook non-interactively. Without a terminal and without `--yes`, a shared settings change is refused. |
| `--force` | `apply` | Overwrites owned files that were edited since the last apply, and foreign files at owned paths. |
| `--user` | `apply`, `diff`, `status`, `remove` | User level instead of the current repository. |
| `--local` | `apply` | Permission entries in `.claude/settings.local.json`. |

## Exit codes

| Code | Meaning |
|---|---|
| 0 | Success (for `diff`: nothing differs) |
| 1 | Failure, drift that stops `apply`, test failures, or (for `diff`) differences |
| 2 | Usage error |

## When `apply` stops

- `claude --version` is below 2.1.295, or Claude Code is not installed.
- An owned file differs from its hash in the lock (someone edited it), or a file Forge does not own sits at a path it would write. The diff is printed; `--force` overwrites.
- `forge.yaml`, a local rule or a settings file is invalid. The message names the file; nothing is written.
