## 4. Ownership

| Category | Paths | Forge does |
|---|---|---|
| **Owned** | `.forge/**`, `.claude/skills/forge-project/**`, `.claude/rules/forge-*.md`; user level: `~/.claude/skills/forge-user/**`, `~/.claude/forge.lock.json` | Regenerates; hashes recorded in the lock |
| **Appended** | `.claude/settings.json` (or `.claude/settings.local.json` with `--local`); `~/.claude/settings.json` | Adds entries only, with consent; each entry recorded in the lock; never changes or removes entries it did not add |
| **Audited** | `biome.json`, `tsconfig.json`, `pyproject.toml`, `package.json`, dependencies | Reads and tests behavior (§11); never writes |
| **Marked block** | a hook manager's config: `.pre-commit-config.yaml`, `lefthook.yml` (`.yaml`, `.lefthook.yml`), `.husky/pre-commit`, `<core.hooksPath>/pre-commit` (§10.3) | Adds one block between markers, only with `--pre-commit` or a yes at the prompt (never with `--yes` alone); recorded in the lock's `blocks`; `remove` takes it out; never changes the rest of the file |
| **Untouched** | `CLAUDE.md`, other `.claude/rules/*`, other `.claude/skills/*`, git config, `.git/hooks/*` (except `.git/hooks/pre-commit`, §10.3), the rest of the hook managers' files, CI files | Nothing |

`forge init` is the only command that creates audited files, and only when they don't exist.
