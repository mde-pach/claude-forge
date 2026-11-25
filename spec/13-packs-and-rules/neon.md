<!-- Generated from the built-in packs by crates/forge-packs/tests/spec_tables.rs; do not edit. Regenerate: FORGE_BLESS=1 cargo test -p forge-packs --test spec_tables -->

## 13. Packs and rules: `neon`

### neon

Neon serverless Postgres, its branches and its CLI.

Detected by: a dependency on one of `@neondatabase/serverless` (npm), `@neondatabase/neon-js` (npm), `@neon/config` (npm), `neonctl` (npm); or a line matching `\.neon\.tech` in a file matching `.env.example`, `.env.sample`, `.env.template`.

No rules of its own.

| Capability (§20) | Description | Delivered | Relevant when | Active when |
|---|---|---|---|---|
| `neon/neon-branch-migration-test` | Trying a schema migration on a throwaway Neon branch of production before it reaches production. | on-demand | The task adds or changes a database migration that will run on a Neon database.<br>The task needs to check how a schema change behaves on production-like data. | wherever its pack is |
