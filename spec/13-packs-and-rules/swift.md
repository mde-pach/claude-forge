<!-- Generated from the built-in packs by crates/forge-packs/tests/spec_tables.rs; do not edit. Regenerate: FORGE_BLESS=1 cargo test -p forge-packs --test spec_tables -->

## 13. Packs and rules: `swift`

### swift

Swift packages and Xcode projects.

Detected by: a file matching `Package.swift`, `**/Package.swift`, `**/*.xcodeproj/project.pbxproj`.

No rules of its own.

### swift/apollo-ios

Apollo iOS, the GraphQL client, and its Swift code generation.

Detected by: a file matching `apollo-codegen-config.json`, `**/apollo-codegen-config.json`; or a line matching `apollographql/apollo-ios\|pod ['"]Apollo['"/]` in a file matching `Package.swift`, `**/Package.swift`, `Podfile`.
Child of `swift`: active only where it is.

No rules of its own.

| Capability (§20) | Description | Delivered | Relevant when | Active when |
|---|---|---|---|---|
| `swift/apollo-ios/apollo-ios-codegen` | Regenerating Apollo iOS's Swift code after a GraphQL operation or schema change, and reading what changed. | on-demand | The task adds or changes a GraphQL query, mutation, subscription or fragment in the iOS app.<br>The task updates the GraphQL schema the iOS app is generated from. | wherever its pack is |

### swift/mcp

Model Context Protocol servers written with the Swift SDK.

Detected by: a line matching `modelcontextprotocol/swift-sdk` in a file matching `Package.swift`, `**/Package.swift`.
Child of `swift`: active only where it is.

No rules of its own.

| Capability (§20) | Description | Delivered | Relevant when | Active when |
|---|---|---|---|---|
| `swift/mcp/mcp-inspector-probe` | Calling the Swift MCP server's tools, resources and prompts through the MCP Inspector's command line and reading its answers. | on-demand | The task adds or changes a tool, resource or prompt of the project's MCP server.<br>The task needs to check what the MCP server answers to a client. | wherever its pack is |
