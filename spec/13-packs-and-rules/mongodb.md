<!-- Generated from the built-in packs by crates/forge-packs/tests/spec_tables.rs; do not edit. Regenerate: FORGE_BLESS=1 cargo test -p forge-packs --test spec_tables -->

## 13. Packs and rules: `mongodb`

### mongodb

MongoDB databases, used through a driver or an ODM.

Detected by: a dependency on one of `mongodb` (npm), `mongoose` (npm), `@typegoose/typegoose` (npm), `pymongo` (PyPI), `motor` (PyPI), `mongoengine` (PyPI), `beanie` (PyPI), `odmantic` (PyPI).

No rules of its own.

| Capability (§20) | Description | Delivered | Relevant when | Active when |
|---|---|---|---|---|
| `mongodb/mongodb-access-pattern-stats` | Measuring a MongoDB database's document sizes, array lengths and workload before changing its schema. | on-demand | The task chooses between embedding and referencing MongoDB data, or splits or merges collections.<br>The task investigates MongoDB documents that grow too large or queries that got slow as the data grew. | wherever its pack is |
| `mongodb/mongodb-inspect` | Reading a MongoDB collection's indexes, fields and sample documents, and running a query against it, before relying on it. | on-demand | The task writes or debugs a MongoDB query or aggregation pipeline against a real database.<br>The task adds or tunes an Atlas Search or Vector Search index or query. | wherever its pack is |
