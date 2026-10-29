## 3. Forge CLI

### 3.1 The binary

- One binary, installed as `forge` and `claude-forge` (§18). Both names run the same program.
- Every command first runs the launcher (§3.3), except `forge internal …` and `forge engine …`, which are always run by the binary that started them.
- Errors go to stderr as one line starting with `forge: `. A refused or failed command exits 1; a usage error exits 2.

### 3.2 Commands

| Command | Behaviour |
|---|---|
| `forge init` | Project level only (`--user` is a usage error). Refuses a repository that already has Forge (a `forge.lock`, or the old layout's lock, §19): `forge: init is for new projects; this one already has forge (use forge apply)`. For each active pack with an `init/` directory, creates each of its files that does not exist yet (`forge: created <path> (from <pack>)`; an existing one: `forge: <path> exists, kept`). A Biome config gets `"off"` for each diagnostic an active pack switches off (`tool_off`, §5.2). Then runs `forge apply` with the same flags. |
| `forge apply` | Plans the setup (§5, §6) and writes it: the rules files, the settings entries, the commit-time check when offered and accepted, and the lock (§7, §12). Then provisions uv and the pinned tools the active checks use (§17) and reports to the person (§3.5). |
| `forge diff` | Prints what `apply` would change, as unified diffs; drift and conflicts as `forge: <path> was modified since the last apply` / `forge: <path> exists and is not a forge file`. Exits 1 when anything differs or drifted, else 0. Writes nothing. |
| `forge status [--json]` | The active packs and why; each active rule and what enforces it here, or why not; disabled, version-gated, switched-off and deprecated rules; file roles; audits; drift; missing tools; tool versions; tool pins; the commit-time check; the CI steps; the person's notes (§3.6). Always exits 0. |
| `forge explain <rule-id>` | Prints the rule file, then whether it is disabled, version-gated, inactive or enforced here, and how; its audit result when it has `tool` targets. Outside a git repository it prints the rule file only. Unknown id: exit 1. |
| `forge remove` | Deletes exactly what the lock records: the files, the settings entries (each shared settings file's change shown and confirmed), Forge's block in a hook manager's file, and the lock (§7.6). A recorded file changed since the last apply is removed anyway, with a warning. A block whose markers were changed is left in place with instructions, and `remove` exits 1. No lock: `forge: no forge.lock; nothing to remove`, exit 1. |
| `forge check [--changed \| --since <ref> \| --all \| --turn <dir>] [--json]` | Runs the active rules' checks (§10). `--all` is the default. Output is for people, notes included; `--json` prints the result as JSON. Exit 0 pass, 1 fail, 3 unverified, 4 advice only; 2 when the check could not run (not a git repository, a broken lock). |
| `forge test [--builtin] [--json]` | Runs the examples of the repository's local rules (its local packs, `.forge/packs/**`, and standalone rules, `.forge/rules/*.yaml`), or with `--builtin` the built-in packs', through the machinery of the pack-example tests (§14.1): each guard, ast-grep, grep and check target on the examples it takes. Prints `PASS`, `FAIL <label>: <why>` or `SKIP <label>: <note>` per example (`<label>`: `<rule> <target> <match\|no_match> example <n>`), then `<n> examples of <m> rules: <p> passed, <f> failed, <s> skipped`; `--json` prints one object per example (`rule`, `target`, `kind`, `index`, `verdict`: `pass`, `fail` with `why`, or `skip` with the `note`), then a summary object. A skipped example is one whose run left a note of a kind that says the rule did not run there (a missing tool, not enforced here, Forge's own failure, the environment): it is told from the result's structure, never from its text. Run by Claude Code (`CLAUDECODE` set), a skip's line has no reason: the note is recorded for `forge status`. Exit 1 when an example fails, else 0; 2 when the rules do not load. A repository without local rules prints `This repository has no local rules.` |
| `forge packs dump [--json] [--repo <dir>] [--cache-dir <dir>]` | Prints the loaded packs as JSON: the built-in ones, or with `--repo` the repository's local packs too. |
| `forge packs lint [--repo <dir>] [--cache-dir <dir>]` | Prints the content lints' findings (§6.6); exits 1 when there are any. |
| `forge hook <event>` | Hidden. Answers one Claude Code hook call: the hook input on stdin, the answer on stdout (§8). |
| `forge internal <builtin> [args…]` | Hidden. Runs a builtin a check command calls (§6.4). |
| `forge engine serve --config <file>` / `forge engine start --root <dir> --session <id>` | Hidden. The types engine daemon and its detached starter (§15.3). |

Local rules are also validated when they load, by every command that loads packs.

### 3.3 Flags of the setup commands

`init`, `apply`, `diff`, `status`, `explain` and `remove` accept:

| Flag | Effect |
|---|---|
| `--user` | The user-level setup (`~/.claude`) instead of the repository's (§7.7). Conflicts with `--local`. |
| `--local` | Settings entries go to `.claude/settings.local.json` instead of `.claude/settings.json` (a repository you do not own). |
| `--force` | `apply` overwrites drifted files, files that exist and are not Forge's, and modified files of the old layout (§19). |
| `--dry-run` | `apply` prints the diff and writes nothing; `init` prints `forge: would create <path> (from <pack>)`; `remove` prints what it would delete. |
| `--yes`, `-y` | Answers yes to confirmations: changes to a shared settings file, deleting the old layout, creating git's pre-commit hook. It does not consent to editing a hook manager's config (§7.6). |
| `--pre-commit` | Consents to adding the commit-time check: git's pre-commit hook, or a block in the hook manager's config (§7.6). |

- A project-level command outside a git repository fails: `forge: not inside a git repository (use --user for the user-level setup)`.
- `apply` is idempotent: a second run prints `forge: up to date` and writes nothing.
- `apply` stops when an owned file changed since the last apply (drift), when a file it would write exists and is not Forge's (conflict), or when a file of the old layout was modified. It prints each with its diff, then `forge: stopped. Re-run with --force to overwrite, or move the files away.`, and exits 1 unless `--force` is given.
- A change to a shared settings file (`.claude/settings.json`, `~/.claude/settings.json`) is shown as a diff and needs a `y` at the prompt, or `--yes`. Without a terminal and without `--yes`, the command fails with `forge: <question> Re-run with --yes to confirm non-interactively.` Answering no prints `forge: cancelled; nothing written` and writes nothing.
- `apply` prints warnings from planning on stderr as `forge: <warning>`: unknown packs in `forge.yaml`, a `disable` naming a rule that is not active, a local rule for a known pack that is not active (§5.6).

### 3.4 The launcher and version pinning

The installed binary is a launcher. Before any command it reads the repository's `forge.lock` and runs the Forge version the lock pins.

- The repository is found from `CLAUDE_PROJECT_DIR`, else the current directory: the nearest directory holding `forge.lock`, else the nearest holding `forge.yaml`. The walk stops at the git root (a directory holding `.git`), so a stray lock above the repository is never used. Outside a Forge repository the launcher does nothing.
- The launcher reads only the lock's top-level `forge = "<version>"` key, an exact semver version (§12).
- When the lock pins this binary's own version, or pins none, the command runs in this process.
- Otherwise the launcher replaces itself (`exec`) with `<cache>/bin/forge-<version>` (§4.3), passing argv, stdin, stdout and the environment through, and sets `FORGE_LAUNCHED_VERSION=<version>` in the child's environment.
- A binary started with `FORGE_LAUNCHED_VERSION` equal to the locked version never execs again. If its own version differs (a mislabelled cache entry), it runs in-process and leaves a note. A nested run in another repository with another pin still launches correctly, because the marker names a version.
- When the pinned version is not in the cache:
  - `forge hook session-start` starts a detached download (§18.3) and runs the installed version now;
  - `forge apply` downloads it, then execs it;
  - every other command and hook runs the installed version and never downloads.
- Each case where the installed version runs instead of the pinned one leaves a note for the person in `<state>/<repo-id>/notes/` (one file per kind, replaced, so repeated hooks leave one note). The texts:
  - `This project pins Forge <locked>, which is not downloaded yet, so Forge <running> ran instead.` followed by ` Forge <locked> is downloading in the background.` or ` The download did not happen: <error>.`, then ` Run `forge apply` to get it now.`
  - `The cached Forge <locked> binary reports version <running>, so it ran as <running>. Delete it and run `forge apply` to download it again.`
  - `Could not start the cached Forge binary <path> (<error>), so the installed version ran instead.`
  - `<error>. The installed Forge version ran instead; fix forge.lock or run `forge apply`.` (an unreadable lock)
- `forge status` and `forge apply` print and clear these notes (§3.6). The SessionStart hook also shows the "not downloaded yet" note to the person as its `systemMessage` (§8.4).

### 3.5 What `apply` reports

After writing, `apply` prints, for the person only:

- `forge: applied (<n> written, <m> deleted, <k> settings file(s) updated)` or `forge: up to date`;
- `forge: installed <tool> <version>, …` for tools it provisioned now, and `forge: note: <text>` for provisioning notes (§17);
- each unsatisfied audit (§11);
- the commit-time check's state: `forge: pre-commit: …` (§7.6);
- `forge: missing tools (checks using them are skipped): <names>`;
- `forge: tool versions (what they cannot do is not enforced):` and one line per tool (§16);
- `forge: note: <text>` for each note left by the launcher, the hooks or the tool probe.

### 3.6 What `status` reports

The text output, in order: `forge <version>: <level> level, <base>`; `applied with forge <v>` or `not applied yet (run forge apply)`; `Packs:` (`<id> (<why>)`: `detected`, `forge.yaml`, `parent of <id>`, `required by <id>`, `user level`; removed ones as `<id>: not active (<why>)`); `Rules: <summary>` then each rule (`(local)` for a local rule) with its enforcement lines and its `not enforced here: …` lines; `Disabled:`; `Inactive (version gate):` (`<id>: needs <deps>`); `Switched off by packs:`; deprecations (`<id>: deprecated, replaced by <id>`); `Files:` (file roles, §15.4); `Audit:`; `Drift:` (or `Drift: none`); `Missing tools:` (or `none`); at project level `Tool versions:` (or `Tool versions: nothing reported`), `Tool pins:` (`as this Forge pins them`, or each `<tool>: <locked> -> <ours>`), `Pre-commit: …`, and the CI steps `claude-forge check --since origin/<default branch>` and `claude-forge check --all`; `Notes:`.

`--json` prints one object with the keys `forge_version`, `level`, `base`, `applied`, `packs`, `inactive_packs`, `counts` (`active`, `enforced`, `not_enforced`), `rules`, `disabled`, `gated`, `switched_off`, `deprecated`, `audits`, `missing_tools`, `tool_versions`, `tool_pins` (`tool`, `locked`, `forge`), `pre_commit`, `drift`, `files`, `notes`, `warnings`.

Drift lines: `<path> modified since the last apply`; `<path> is not a forge file`; `rule <id> changed since the last apply`; file-role changes; `<file>: missing <path> <value>` (a recorded settings entry that is gone); pre-commit block changes; `the repository has forge 0.5's layout (forge apply moves it to the new one)`; `<n> file(s) differ from what apply would write (forge diff)`.

### Tests

- Commands, flags, idempotence, drift, dry run, remove, Claude Code gate, migration: `crates/forge-cli/tests/apply.rs`.
- Usage errors (exit 2), consent without a terminal, `--local`, diff and re-added entries, init, user level, `status` text and `--json` keys and tool-version lines, explain: `crates/forge-cli/tests/spec_cli.rs`.
- Launcher decisions, exec, loop marker, notes: `crates/forge-launcher/src/{lib,note,policy}.rs` (unit); `crates/forge-launcher/tests/exec.rs`.
- Lock reading by the launcher: `crates/forge-launcher/src/lock.rs` (unit); `crates/forge-launcher/tests/lock_differential.rs`.
- Repository root discovery: `crates/forge-home/src/lib.rs` (unit).
- `forge test`: `crates/forge-cli/tests/forge_test.rs` (local rules and a local pack: pass, fail, skip, with and without `CLAUDECODE`), `crates/forge-cli/tests/check.rs::check_examples` (`--builtin --json`).
- `forge check`: §10.
- `forge packs lint`: `crates/forge-packs/tests/lints.rs`.
