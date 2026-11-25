<!-- Generated from the built-in packs by crates/forge-packs/tests/spec_tables.rs; do not edit. Regenerate: FORGE_BLESS=1 cargo test -p forge-packs --test spec_tables -->

## 13. Packs and rules: `grafana`

### grafana

Grafana dashboards as code and the Grafana observability stack (Loki, Alloy).

Detected by: a dependency on one of `grafanalib` (PyPI), `grafana-foundation-sdk` (PyPI), `@grafana/grafana-foundation-sdk` (npm); or a file matching `**/grafana.ini`, `**/provisioning/dashboards/*.yaml`, `**/provisioning/dashboards/*.yml`, `**/*.alloy`, `**/loki-config.yaml`, `**/loki-config.yml`, `**/loki.yaml`; or a line matching `^\s*image:\s*["']?(docker\.io/)?grafana/(grafana\|grafana-oss\|grafana-enterprise\|loki\|alloy\|promtail)\b` in a file matching `**/compose*.yaml`, `**/compose*.yml`, `**/docker-compose*.yaml`, `**/docker-compose*.yml`; or a line matching `^\s*"schemaVersion"\s*:\s*[0-9]+` in a file matching `**/dashboards/**/*.json`, `**/grafana/**/*.json`; or a line matching `grafonnet` in a file matching `**/*.jsonnet`, `**/*.libsonnet`; or a line matching `^\s*resource\s+"grafana_(dashboard\|folder)"` in a file matching `**/*.tf`.

No rules of its own.

| Capability (§20) | Description | Delivered | Relevant when | Active when |
|---|---|---|---|---|
| `grafana/dashboard-roundtrip` | Loading a dashboard's JSON into a Grafana instance through its HTTP API and reading it back, to see that Grafana accepts and renders it. | on-demand | The task creates or changes a Grafana dashboard, its panels, queries or variables. | wherever its pack is |
| `grafana/loki-query` | Searching a service's logs with LogQL through Loki's HTTP API or logcli. | on-demand | The task is to find the cause of an error or a behaviour seen in a running service's logs.<br>The task needs logs from Loki read, filtered or counted. | wherever its pack is |
