<!-- Generated from the built-in packs by crates/forge-packs/tests/spec_tables.rs; do not edit. Regenerate: FORGE_BLESS=1 cargo test -p forge-packs --test spec_tables -->

## 13. Packs and rules: `testcontainers`

### testcontainers

Tests that start the services they need with Testcontainers, so they need a Docker daemon.

Detected by: a dependency on one of `testcontainers` (PyPI), `testcontainers` (npm), `@testcontainers/*` (npm).

Context (in `.claude/rules/forge-*.md`, §7):

- Integration tests start each third-party service they need (database, broker, cache) with Testcontainers, from a fixture or a setup file; they do not rely on a running compose stack.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `testcontainers/sandbox-docker` | Claude's test runs reach Docker through the project's .claude/settings.local.json: on macOS the sandbox allows this machine's Docker socket (rootless Docker or Podman first) and the containers' published ports on localhost, the socket giving the tests control of the Docker daemon; on Linux, where a sandboxed command cannot reach those ports, the usual test commands (pytest, vitest, jest, bun test, npm test) run outside the sandbox, after Claude Code's permission check. | any | sandbox `network.allowUnixSockets` = `["{docker-socket}"]` (macOS)<br>sandbox `network.allowLocalBinding` = `true` (macOS)<br>sandbox `excludedCommands` = `["pytest *","python -m pytest *","python3 -m pytest *","uv run pytest *","poetry run pytest *","vitest *","npx vitest *","pnpm vitest *","pnpm exec vitest *","yarn vitest *","bunx vitest *","jest *","npx jest *","bun test *","npm test *","npm run test *","pnpm test *","yarn test *"]` (Linux) |
