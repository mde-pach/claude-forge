<!-- Generated from the built-in packs by crates/forge-packs/tests/spec_tables.rs; do not edit. Regenerate: FORGE_BLESS=1 cargo test -p forge-packs --test spec_tables -->

## 13. Packs and rules: `rust`

### rust

Rust crates and Cargo workspaces.

Detected by: a file matching `Cargo.toml`, `**/Cargo.toml`.

No rules of its own.

### rust/mcp

Model Context Protocol servers written with the Rust SDK (rmcp).

Detected by: a line matching `^[ \t]*(rmcp[ \t]*=\|\[(workspace\.)?dependencies\.rmcp\])` in a file matching `Cargo.toml`, `**/Cargo.toml`.
Child of `rust`: active only where it is.

No rules of its own.

| Capability (§20) | Description | Delivered | Relevant when | Active when |
|---|---|---|---|---|
| `rust/mcp/mcp-inspector-probe` | Calling the Rust MCP server's tools, resources and prompts through the MCP Inspector's command line and reading its answers. | on-demand | The task adds or changes a tool, resource or prompt of the project's MCP server.<br>The task needs to check what the MCP server answers to a client. | wherever its pack is |
