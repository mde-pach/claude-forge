<!-- Generated from the built-in packs by crates/forge-packs/tests/spec_tables.rs; do not edit. Regenerate: FORGE_BLESS=1 cargo test -p forge-packs --test spec_tables -->

## 13. Packs and rules: `graphql`

### graphql

GraphQL schemas and operations, and the Apollo GraphOS tooling around them (Rover, Router, MCP Server).

Detected by: a file matching `**/*.graphql`, `**/*.graphqls`, `**/*.gql`, `**/supergraph.yaml`, `**/supergraph-config.yaml`, `**/router.yaml`, `**/router.yml`; or a line matching `apollographql/(router\|apollo-mcp-server)\|\bapollo-mcp-server\b` in a file matching `**/Dockerfile`, `**/compose*.yaml`, `**/compose*.yml`, `**/docker-compose*.yaml`, `**/docker-compose*.yml`, `**/*.yaml`, `**/*.yml`, `.mcp.json`.

No rules of its own.

### graphql/apollo-federation

Federated GraphQL supergraphs composed with Apollo Rover.

Detected by: a dependency on one of `@apollo/subgraph` (npm), `@apollo/gateway` (npm), `@apollo/rover` (npm); or a file matching `**/supergraph.yaml`, `**/supergraph-config.yaml`; or a line matching `@link\(\s*url:\s*"https://specs\.apollo\.dev/federation/` in a file matching `**/*.graphql`, `**/*.graphqls`.
Child of `graphql`: active only where it is.

No rules of its own.

| Capability (§20) | Description | Delivered | Relevant when | Active when |
|---|---|---|---|---|
| `graphql/apollo-federation/rover-dev` | Composing the supergraph locally with rover dev and querying it end to end, to check a federated change. | on-demand | The task changes a subgraph's schema, entities or resolvers in a federated GraphQL graph.<br>The task needs a federated query checked end to end across subgraphs. | wherever its pack is |
| `graphql/apollo-federation/schema-search` | Finding types and fields in a GraphOS graph's schema with rover schema search and describe, without reading the whole schema. | on-demand | The task writes a new query, mutation or operation for an API.<br>The task needs to find which type or field of a GraphQL schema holds some data. | wherever its pack is |

### graphql/apollo-mcp-server

Apollo MCP Server, exposing GraphQL operations as MCP tools.

Detected by: a line matching `apollographql/apollo-mcp-server\|\bapollo-mcp-server\b\|rover\s+dev\b.*--mcp` in a file matching `**/Dockerfile`, `**/compose*.yaml`, `**/compose*.yml`, `**/docker-compose*.yaml`, `**/docker-compose*.yml`, `**/*.yaml`, `**/*.yml`, `.mcp.json`, `**/package.json`, `**/Makefile`.
Child of `graphql`: active only where it is.

No rules of its own.

| Capability (§20) | Description | Delivered | Relevant when | Active when |
|---|---|---|---|---|
| `graphql/apollo-mcp-server/mcp-run` | Running Apollo MCP Server from its config and calling its tools, to see which tools the GraphQL operations become and what they return. | on-demand | The task adds or changes the GraphQL operations or configuration that an MCP server exposes as tools.<br>The task is to fix or check what an MCP tool returns. | wherever its pack is |

### graphql/apollo-router

Apollo Router (GraphOS Router) configuration for federated GraphQL.

Detected by: a line matching `^(supergraph\|cors\|headers\|authentication\|authorization\|telemetry\|include_subgraph_errors\|traffic_shaping\|coprocessor\|rhai\|plugins\|health_check\|sandbox\|homepage\|apq\|limits\|override_subgraph_url):` in a file matching `**/router.yaml`, `**/router.yml`; or a line matching `ghcr\.io/apollographql/router` in a file matching `**/Dockerfile`, `**/compose*.yaml`, `**/compose*.yml`, `**/docker-compose*.yaml`, `**/docker-compose*.yml`, `**/*.yaml`, `**/*.yml`.
Child of `graphql`: active only where it is.

No rules of its own.

| Capability (§20) | Description | Delivered | Relevant when | Active when |
|---|---|---|---|---|
| `graphql/apollo-router/router-dev` | Running Apollo Router locally with the changed router.yaml and sending requests to see what the configuration really does. | on-demand | The task changes the router's configuration, such as authentication, CORS, headers, caching, limits or telemetry. | wherever its pack is |
