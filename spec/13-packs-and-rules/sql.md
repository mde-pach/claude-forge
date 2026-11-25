<!-- Generated from the built-in packs by crates/forge-packs/tests/spec_tables.rs; do not edit. Regenerate: FORGE_BLESS=1 cargo test -p forge-packs --test spec_tables -->

## 13. Packs and rules: `sql`

### sql

Plain SQL, its migrations (Flyway, Supabase, dbmate, sqitch, Atlas) and its sqlfluff style.

Detected by: a file matching `flyway.conf`, `flyway.toml`, `conf/flyway.conf`, `supabase/config.toml`, `sqitch.plan`, `atlas.hcl`, `db/migrations/*.sql`, `.sqlfluff`; or a line matching `^\[tool\.sqlfluff(\]\|\.)` in a file matching `pyproject.toml`; or a line matching `^\[sqlfluff(\]\|:)` in a file matching `setup.cfg`, `tox.ini`.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `sql/applied-migrations` | SQL migrations already on the default branch (file role @sql-migrations) are not modified or deleted; they may already be applied (Flyway checksums them), so a change goes in a new migration. A migration this branch added and that is not merged yet may still be edited. | any | check migration: `forge_applied_migrations @sql-migrations` |
| `sql/migration-lint` | New or changed Postgres migrations of Flyway, Supabase, dbmate, sqitch and Atlas (file role @sql-migrations) do not break the running app during a deploy (no column or table renames, type changes, new required columns or DROP DATABASE); a finding is fixed, not silenced with squawk-ignore comments or excluded_rules. | any: `@sql-migrations` | check migration: `forge_squawk_role @sql-migrations` (when `@sql-migrations` changes) |
| `sql/no-grant-all` | A migration does not grant every privilege on every object of a schema (GRANT ALL [PRIVILEGES] ON ALL TABLES / SEQUENCES / FUNCTIONS IN SCHEMA): the role gets DELETE, TRUNCATE and the rest on tables created later too. Grant the privileges the role needs on the objects it uses (GRANT SELECT, INSERT ON orders TO app). | any: `@sql-migrations` | grep forbid `grant[[:space:]]+all([[:space:]]+privileges)?[[:space:]]+on[[:space:]]+all[[:space:]]+(tables\|sequences\|functions\|routines\|procedures)[[:space:]]+in[[:space:]]+schema` (ignoring case) |
| `sql/sqlfluff` | Changed .sql files pass the project's sqlfluff lint when the project configures it (.sqlfluff, [tool.sqlfluff] in pyproject.toml, [sqlfluff] in setup.cfg or tox.ini: the dialect and rules are the project's). | any: `**/*.sql` | check lint: `forge_sqlfluff` (when `**/*.sql`, `.sqlfluff`, `pyproject.toml`, `setup.cfg` or `tox.ini` changes)<br>applies where `sqlfluff >=2` (PyPI) declared |

| Capability (§20) | Description | Delivered | Relevant when | Active when |
|---|---|---|---|---|
| `sql/query-plan` | Reading a query's execution plan with EXPLAIN on a development database, to see how it runs and which index it uses. | on-demand | The task is to make a SQL query faster or to find why it is slow.<br>The task adds or changes a database index. | wherever its pack is |

### sql/atlas

Atlas, the project's database schema and migration tool.

Detected by: a file matching `atlas.hcl`.
Child of `sql`: active only where it is.

No rules of its own.

| Capability (§20) | Description | Delivered | Relevant when | Active when |
|---|---|---|---|---|
| `sql/atlas/migrations` | Changing a database schema managed by Atlas, through generated versioned migrations. | inject | The task changes the database schema, such as a table, a column, an index or a constraint.<br>The task adds or changes a database migration. | wherever its pack is |

### sql/atlas/versioned

Atlas with versioned migrations (a migration block in atlas.hcl).

Detected by: a line matching `^\s*migration\s*\{` in a file matching `atlas.hcl`.
Child of `sql/atlas`: active only where it is.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `sql/atlas/versioned/no-schema-apply` | This project changes its database schema through versioned migrations, so atlas schema apply (which changes a database straight to the desired schema, outside the migrations directory) asks first. Generate a migration with atlas migrate diff and apply it with atlas migrate apply. | any | guard ask: command `{name: atlas, sub: [schema, apply]}` |
