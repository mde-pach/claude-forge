<!-- Generated from the built-in packs by crates/forge-packs/tests/spec_tables.rs; do not edit. Regenerate: FORGE_BLESS=1 cargo test -p forge-packs --test spec_tables -->

## 13. Packs and rules: `testcontainers`

### testcontainers

Tests that start the services they need with Testcontainers, so they need a Docker daemon.

Detected by: a dependency on one of `testcontainers` (PyPI), `testcontainers` (npm), `@testcontainers/*` (npm).

Context (in `.claude/rules/forge-*.md`, §7):

- Integration tests start each third-party service they need (database, broker, cache) with Testcontainers, from a fixture or a setup file; they do not rely on a running compose stack.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `testcontainers/sandbox-docker` | Claude's sandboxed test runs reach Docker: this machine's Docker socket (rootless Docker or Podman first) and the containers' published ports on localhost are allowed, in the project's .claude/settings.local.json. The socket gives code in the tests control of the Docker daemon. | any | sandbox `network.allowUnixSockets` = `["{docker-socket}"]`<br>sandbox `network.allowLocalBinding` = `true` |
