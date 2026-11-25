<!-- Generated from the built-in packs by crates/forge-packs/tests/spec_tables.rs; do not edit. Regenerate: FORGE_BLESS=1 cargo test -p forge-packs --test spec_tables -->

## 13. Packs and rules: `supabase`

### supabase

Supabase projects, their local stack, database migrations and client libraries.

Detected by: a dependency on one of `@supabase/supabase-js` (npm), `@supabase/ssr` (npm), `supabase` (npm), `supabase` (PyPI); or a file matching `supabase/config.toml`, `*/supabase/config.toml`.

No rules of its own.

| Capability (§20) | Description | Delivered | Relevant when | Active when |
|---|---|---|---|---|
| `supabase/supabase-schema-sync` | Changing the Supabase schema on the local stack, then generating the migration from it and checking the migration list. | on-demand | The task changes the Supabase database schema, such as a table, a column, a policy, a function or a trigger.<br>The task adds or fixes a Supabase migration. | a file matches `supabase/config.toml`, `*/supabase/config.toml` |
