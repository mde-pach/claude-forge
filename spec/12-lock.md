## 12. Lock

`.forge/forge.lock.json` and `~/.claude/forge.lock.json`:

```json
{"forge_version":"0.5.0","claude_code_min":"2.1.284",
 "rules":{"<rule-id>":"<sha256 of rule file>"},
 "files":{"<path>":"<sha256>"},
 "settings":[{"file":".claude/settings.json","path":"permissions.ask","value":"Edit(.forge/**)"}]}
```

- `diff` and `status` report owned files whose hash differs, appended entries that are missing, and rules whose source changed since the last apply.
- `remove` deletes listed files and removes exactly the listed settings entries.
- **Validation.** Every command reading the lock validates it against its schema and refuses it as a whole (exit 1, nothing written or deleted) when a path is not one Forge writes: `files` keys must be owned paths (§4) or git's `hooks/pre-commit` (`git rev-parse --git-path`), `settings[].file` a settings file of that level, `blocks[].file` the file `hookManager` names for its `kind` (`.pre-commit-config.yaml`, `.husky/pre-commit`, `lefthook.yml` / `.yaml` / `.lefthook.yml`, `<core.hooksPath>/pre-commit`); all relative, without `.` or `..` segments, and inside the repository (or `~/.claude`) once symbolic links are resolved.
- **`roles`** (present only when an active rule names a file role): `{"<role>": ["<glob>", "!<glob>"]}`, the effective patterns at the last apply (§15.4); `status` and `diff` report a role whose patterns changed since (a tool config or `forge.yaml` changed).
- **`blocks`** (present only when there is one): `[{file, kind, created, hash}]`, Forge's marked block in a hook manager's file (§10.3); `hash` is the sha256 of the block's lines, indentation removed.
- **Owned-file hashes** are sha256 of the content, except for `.json` files, which are hashed with spaces, tabs, CR and LF removed, and the scripts in `.forge/helpers/`, hashed also without `'`, `"`, `` ` ``, `,`, `;`, `(` and `)`: a formatter reflowing Forge's JSON, or rewriting its helper scripts (`ruff format .`, `biome check --write .`: layout, quote style, trailing commas, semicolons, parentheses), is not drift; any other change is. (A change made only of those characters, such as moving a parenthesis, is not detected: the price of letting formatters rewrite the scripts.) A file whose owned hash matches is not rewritten by `apply`. The lock is compared as JSON (formatting ignored).
- **`.forge/manifest.sha256`** (project level) repeats the owned-file hashes of `.forge/` and `.claude/` as `<sha256>  <path>` lines (sha256sum format) for `check.sh`. Non-JSON lines verify with `sha256sum -c`; JSON and helper lines need the normalised content, so `check.sh` hashes each file itself (`sha256sum`, `shasum -a 256` or `openssl`). The manifest and the lock do not hash themselves; `remove` deletes both.
- **Formatting.** During `apply`, the JSON files Forge generates (plugin manifest, `hooks.json`, the lock, and a `.claude/settings.json` that Forge creates) and its JavaScript helpers (`.forge/helpers/*.mjs`, written free of Biome's `recommended` lint findings) are formatted with the project's own Biome when installed (`biome format --stdin-file-path`), so the project's `biome ci .` passes; without Biome, they, the context files (`.claude/rules/forge-*.md`), the ast-grep YAML files and the JavaScript helpers are formatted with the project's own Prettier when installed (`prettier --stdin-filepath`), so `prettier --check .` passes. An existing settings file keeps its own indentation for appended entries. Project-owned files are never formatted.
