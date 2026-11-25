<!-- Generated from the built-in packs by crates/forge-packs/tests/spec_tables.rs; do not edit. Regenerate: FORGE_BLESS=1 cargo test -p forge-packs --test spec_tables -->

## 13. Packs and rules: `datadog`

### datadog

Services monitored with Datadog (its tracers, agent configuration or monitors as code).

Detected by: a dependency on one of `ddtrace` (PyPI), `datadog` (PyPI), `dd-trace` (npm), `@datadog/browser-rum` (npm); or a file matching `**/datadog.yaml`, `**/datadog.yml`; or a line matching `github\.com/DataDog/dd-trace-go\|gopkg\.in/DataDog/dd-trace-go` in a file matching `**/go.mod`; or a line matching `com\.datadoghq` in a file matching `**/pom.xml`, `**/build.gradle`, `**/build.gradle.kts`; or a line matching `^\s*resource\s+"datadog_` in a file matching `**/*.tf`; or a line matching `\bDD_(SERVICE\|ENV\|AGENT_HOST\|TRACE_AGENT_URL\|API_KEY\|SITE)\b` in a file matching `**/Dockerfile`, `**/*.yaml`, `**/*.yml`.

No rules of its own.

| Capability (§20) | Description | Delivered | Relevant when | Active when |
|---|---|---|---|---|
| `datadog/investigate` | Searching a service's production logs, traces and APM stats in Datadog with the pup CLI. | on-demand | The task is to find the cause of an error, an outage or slowness seen in production.<br>The task needs production logs, traces or request metrics of a service read. | wherever its pack is |
| `datadog/monitor-audit` | Listing and inspecting a service's Datadog monitors with the pup CLI, to find noisy, muted or ownerless ones. | on-demand | The task adds, changes or reviews alerting or monitors for a service.<br>The task is about alerts that fire too often, too rarely or for the wrong reason. | wherever its pack is |

### datadog/llmobs

LLM applications traced with Datadog LLM Observability.

Detected by: a line matching `\bLLMObs\b` in a file matching `**/*.py`; or a line matching `\.llmobs\b` in a file matching `**/*.ts`, `**/*.js`, `**/*.mjs`; or a line matching `\bDD_LLMOBS_ENABLED\b` in a file matching `**/Dockerfile`, `**/*.yaml`, `**/*.yml`, `.env.example`.
Child of `datadog`: active only where it is.

No rules of its own.

| Capability (§20) | Description | Delivered | Relevant when | Active when |
|---|---|---|---|---|
| `datadog/llmobs/trace-rca` | Finding why an LLM application fails in production from its Datadog LLM Observability spans, errors and evaluations. | on-demand | The task is to find why an LLM application or agent gives bad answers or fails in production. | wherever its pack is |
