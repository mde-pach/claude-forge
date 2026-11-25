<!-- Generated from the built-in packs by crates/forge-packs/tests/spec_tables.rs; do not edit. Regenerate: FORGE_BLESS=1 cargo test -p forge-packs --test spec_tables -->

## 13. Packs and rules: `confluent-flink`

### confluent-flink

Flink SQL statements run on Confluent Cloud.

Detected by: a line matching `^\s*resource\s+"confluent_flink_(statement\|compute_pool)"` in a file matching `**/*.tf`; or a line matching `'(key\|value)\.format'\s*=\s*'(avro\|json\|proto)-registry'` in a file matching `**/*.sql`.

No rules of its own.

| Capability (§20) | Description | Delivered | Relevant when | Active when |
|---|---|---|---|---|
| `confluent-flink/flink-explain` | Checking a Flink SQL statement on Confluent Cloud with EXPLAIN, then running it and reading its status and output with the confluent CLI. | on-demand | The task writes or changes a Flink SQL statement or query.<br>The task investigates a Flink statement that fails, is stuck or produces wrong output. | wherever its pack is |
