<!-- Generated from the built-in packs by crates/forge-packs/tests/spec_tables.rs; do not edit. Regenerate: FORGE_BLESS=1 cargo test -p forge-packs --test spec_tables -->

## 13. Packs and rules: `sql`

### sql

Plain SQL, its migrations (Flyway, Supabase, dbmate, sqitch, Atlas) and its sqlfluff style.

Detected by: a file matching `flyway.conf`, `flyway.toml`, `conf/flyway.conf`, `supabase/config.toml`, `sqitch.plan`, `atlas.hcl`, `db/migrations/*.sql`, `.sqlfluff`; or a line matching `^\[tool\.sqlfluff(\]\|\.)` in a file matching `pyproject.toml`; or a line matching `^\[sqlfluff(\]\|:)` in a file matching `setup.cfg`, `tox.ini`.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `sql/applied-migrations` | SQL migrations already in the base (file role @sql-migrations) are not modified or deleted; they may already be applied (Flyway checksums them), so a change goes in a new migration. | any | check migration: `forge_applied_migrations @sql-migrations` |
| `sql/migration-lint` | New or changed Postgres migrations of Flyway, Supabase, dbmate, sqitch and Atlas (file role @sql-migrations) do not break the running app during a deploy (no column or table renames, type changes, new required columns or DROP DATABASE); a finding is fixed, not silenced with squawk-ignore comments or excluded_rules. | any: `@sql-migrations` | check migration: `forge_squawk_role @sql-migrations` (when `@sql-migrations` changes) |
| `sql/sqlfluff` | Changed .sql files pass the project's sqlfluff lint when the project configures it (.sqlfluff, [tool.sqlfluff] in pyproject.toml, [sqlfluff] in setup.cfg or tox.ini: the dialect and rules are the project's). | any: `**/*.sql` | check lint: `forge_sqlfluff` (when `**/*.sql`, `.sqlfluff`, `pyproject.toml`, `setup.cfg` or `tox.ini` changes)<br>applies where `sqlfluff >=2` (PyPI) declared |
