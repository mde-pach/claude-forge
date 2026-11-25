<!-- Generated from the built-in packs by crates/forge-packs/tests/spec_tables.rs; do not edit. Regenerate: FORGE_BLESS=1 cargo test -p forge-packs --test spec_tables -->

## 13. Packs and rules: `prometheus`

### prometheus

Prometheus metrics, PromQL alerting and recording rules.

Detected by: a dependency on one of `prom-client` (npm), `prometheus-client` (PyPI); or a file matching `**/prometheus.yml`, `**/prometheus.yaml`; or a line matching `^kind:[ \t]*["']?PrometheusRule["']?[ \t]*$` in a file matching `**/*.yaml`, `**/*.yml`; or a line matching `^[ \t]*-?[ \t]*(alert\|record):[ \t]*\S` in a file matching `**/*.rules.yml`, `**/*.rules.yaml`, `**/rules/*.yml`, `**/rules/*.yaml`, `**/alerts/*.yml`, `**/alerts/*.yaml`.

No rules of its own.

| Capability (§20) | Description | Delivered | Relevant when | Active when |
|---|---|---|---|---|
| `prometheus/promql-query` | Running PromQL against the Prometheus-compatible backend to see which metrics and labels exist and what an expression returns. | on-demand | The task writes or changes a PromQL expression in an alert, a recording rule or a dashboard.<br>The task needs to know which metrics or labels exist, or what values a metric has. | wherever its pack is |
