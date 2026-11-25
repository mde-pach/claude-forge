<!-- Generated from the built-in packs by crates/forge-packs/tests/spec_tables.rs; do not edit. Regenerate: FORGE_BLESS=1 cargo test -p forge-packs --test spec_tables -->

## 13. Packs and rules: `netlify`

### netlify

Sites and functions deployed on Netlify.

Detected by: a dependency on one of `netlify-cli` (npm), `@netlify/functions` (npm), `@netlify/edge-functions` (npm), `@netlify/database` (npm); or a file matching `netlify.toml`.

No rules of its own.

### netlify/database

Netlify Database, the Postgres database Netlify provisions, and its migrations.

Detected by: a dependency on `@netlify/database` (npm); or a file matching `netlify/database/migrations/**`.
Child of `netlify`: active only where it is.

No rules of its own.

| Capability (§20) | Description | Delivered | Relevant when | Active when |
|---|---|---|---|---|
| `netlify/database/netlify-db-local` | Applying Netlify Database migrations to the local database and querying it with the Netlify CLI. | on-demand | The task adds or changes a Netlify Database migration.<br>The task writes or debugs a query against the project's Netlify Database. | wherever its pack is |
