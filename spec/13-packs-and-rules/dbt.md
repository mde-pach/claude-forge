<!-- Generated from the built-in packs by crates/forge-packs/tests/spec_tables.rs; do not edit. Regenerate: FORGE_BLESS=1 cargo test -p forge-packs --test spec_tables -->

## 13. Packs and rules: `dbt`

### dbt

dbt projects (SQL models, tests and sources).

Detected by: a file matching `**/dbt_project.yml`, `**/dbt_project.yaml`.

No rules of its own.

| Capability (§20) | Description | Delivered | Relevant when | Active when |
|---|---|---|---|---|
| `dbt/dbt-show` | Checking a dbt model's real output rows with dbt show while writing or changing it. | on-demand | The task adds or changes a dbt model or its SQL transformation.<br>The task is to find why a dbt model returns wrong, missing or duplicated rows. | wherever its pack is |
