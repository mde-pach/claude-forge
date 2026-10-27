### sql

Detected by `flyway.conf`, `flyway.toml`, `conf/flyway.conf`, `supabase/config.toml`, `sqitch.plan`, `atlas.hcl`, `db/migrations/*.sql`, `.sqlfluff`, or a `[tool.sqlfluff]` (`pyproject.toml`) / `[sqlfluff]` (`setup.cfg`, `tox.ini`) section. Not tied to a language.

| Rule | Targets |
|---|---|
| `sql/applied-migrations` | `check` migration: `forge_applied_migrations @sql-migrations` (§15.4): a migration that exists in the base is not modified or deleted (not with `--all`) |
| `sql/migration-lint` | `check` migration: `forge_squawk_role @sql-migrations` (§13.1) on the changed migrations (the newest one with `--all`) whose tool targets Postgres, read from literal config only: Supabase always; Flyway `flyway.url` / `url` `jdbc:postgresql:`; sqitch `sqitch.conf` `engine = pg`; Atlas `atlas.hcl` `url` / `dev` `postgres://` or `docker://postgres`; dbmate the `DATABASE_URL` scheme. Other dialects are not linted |
| `sql/sqlfluff` | `check` lint, where `sqlfluff` is a declared PyPI dependency (`when`): `forge_sqlfluff {files}`, when the project configures sqlfluff (`.sqlfluff`, `[tool.sqlfluff]` in `pyproject.toml`, `[sqlfluff]` in `setup.cfg` / `tox.ini`): the project's `sqlfluff lint --format github-annotation-native` on the changed `.sql` files (the project with `--all`), `path:line:column: CODE: message` |
