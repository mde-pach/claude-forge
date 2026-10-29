## 4. Ownership and layout

### 4.1 The repository

```
<repo>/
├── forge.yaml                    input: the project's configuration (optional, §5.5)
├── forge.lock                    owned: written by forge apply (§12)
├── .forge/
│   ├── packs/<id>/…              input: local packs (optional, §5.6)
│   └── rules/<name>.yaml         input: local rules (optional, §5.6)
└── .claude/
    ├── settings.json             appended: hook and permission entries (§7.2, §7.3)
    └── rules/forge-<pack>.md     owned: the context the agent reads (§7.1)
```

Nothing Forge generates goes in `.forge/`. There are no scripts, helpers, manifests or state in the repository; `apply` deletes empty directories it leaves under `.forge/`.

### 4.2 Categories

| Category | Paths | Forge does |
|---|---|---|
| **Owned** | `forge.lock`; `.claude/rules/forge-*.md`; git's pre-commit hook when Forge created it (§7.6). User level: `~/.claude/forge-user.lock`. | Regenerates them. Each owned file's sha256 is recorded in the lock's `[files]`, except the lock itself. |
| **Input** | `forge.yaml`, `.forge/packs/**`, `.forge/rules/**` | Reads them; never writes them. |
| **Appended** | `.claude/settings.json` (or `.claude/settings.local.json` with `--local`); user level: `~/.claude/settings.json` | Adds entries only, with consent for a shared file; records each entry in the lock's `[[settings]]`; never changes or removes an entry it did not add. A settings file it created and that ends up empty is deleted on `remove`. |
| **Marked block** | A hook manager's config: `.pre-commit-config.yaml`, `lefthook.yml` (and its variants), `.husky/pre-commit`, `<core.hooksPath>/pre-commit` (§7.6) | Adds one block between `# forge: begin (forge remove deletes this block)` and `# forge: end`, only with `--pre-commit` or a yes at the prompt; records it in the lock's `[[blocks]]`; `remove` takes it out; never changes the rest of the file. |
| **Audited** | Tool configs (`biome.json`, `pyproject.toml`, `tsconfig.json`, ESLint configs, …), `package.json`, dependencies | Reads them and tests what they enforce (§11); never writes them. |
| **Created once** | The active packs' `init/` templates | `forge init` creates each one only when it does not exist (§3.2). |
| **Untouched** | `CLAUDE.md`, other `.claude/rules/*`, `.claude/skills/*`, git config, other hooks, CI files | Nothing. |

The lock is validated before use. A lock that names an owned file Forge does not write (outside `.claude/rules/forge-*.md` and git's pre-commit hook), a settings file other than the level's, or a block in a file Forge does not add blocks to, is refused as a whole: `forge.lock: refused: <problems>`. Paths must be relative, without `.`, `..`, empty segments or backslashes, and must resolve inside the repository with symbolic links followed.

### 4.3 The per-user cache and state

Forge's runtime files live in two per-user roots, the same on macOS and Linux:

- the **cache root** (re-downloadable, safe to delete): `$FORGE_HOME`, else `$XDG_CACHE_HOME/forge`, else `~/.cache/forge`;
- the **state root** (per repository and per session): `$FORGE_HOME/state`, else `$XDG_STATE_HOME/forge`, else `~/.local/state/forge`.

A relative or empty value of these variables is ignored. With none of them set, Forge cannot run (`cannot find a home directory: set HOME or FORGE_HOME to an absolute path`); a hook then exits 0 silently.

```
<cache>/bin/forge-<version>              the Forge binaries the launcher runs (§3.4)
<cache>/tools/<name>/<version>/          tools Forge provisions, sha256-checked (§17)
<cache>/path/                            only uv and uvx: put on the agent's PATH (§17.6)
<cache>/packs/                           the parsed-packs cache, built-in and local separately (§5.1)
<cache>/engine/<repo-id>.sock            the types engine's socket (§15.3)
<cache>/registry/, <cache>/infra/        cached answers of package registries and infra helpers
<state>/<repo-id>/                       per repository: hook rule cache, notes, last results, engine log
<state>/<repo-id>/notes/                 notes for the person, read and cleared by forge status / apply
<state>/<repo-id>/sessions/<session>/    per session: the turn snapshot, Bash snapshots, Stop counter, reviews
<state>/facts/                           what tools are and have (versions), shared by every repository
<state>/user/                            the user-level hooks' state
```

- `<repo-id>` is the first 16 hex digits of the sha256 of the repository root's physical path (symbolic links resolved). Two clones get two ids.
- Every path component that comes from outside (a version, a tool name, a session id) is checked before use: `[A-Za-z0-9._+-]`, 1 to 128 bytes, starting with a letter or digit (no `.` in a session id), so it cannot leave its directory.
- What each hook keeps in the state root is in §8.

### 4.4 The agent and the per-user roots

The agent has no reason to look into the cache or state roots, and nothing points it there. When it does, the PreToolUse hook denies the call with a neutral reason (§9.3). A path counts as internal when it is inside either root, as written (with `..` removed) or with symbolic links resolved, so a link in the repository pointing into the cache is caught. Hard links cannot be detected by path.

The refusal fires for the tools the hook entries match (§7.3): at project level Bash, Read, Edit, Write, MultiEdit, NotebookEdit, Grep and Glob; at user level Bash only, so outside a Forge project only Bash commands are refused.

### Tests

- Owned files, appended entries, refusal of drift and conflicts, remove: `crates/forge-cli/tests/apply.rs`; entries the person already had kept unrecorded, a settings file Forge created deleted by `remove`, stale owned files deleted, refused locks: `crates/forge-cli/tests/spec_cli.rs`.
- Settings files edited in place, the person's text kept, emptied containers pruned: `crates/forge-render/src/settings.rs` (unit).
- Marked blocks: `crates/forge-render/src/precommit.rs` (unit).
- Cache and state roots, repository ids, component checks, internal paths: `crates/forge-home/src/lib.rs` (unit).
