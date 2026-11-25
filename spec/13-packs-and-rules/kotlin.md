<!-- Generated from the built-in packs by crates/forge-packs/tests/spec_tables.rs; do not edit. Regenerate: FORGE_BLESS=1 cargo test -p forge-packs --test spec_tables -->

## 13. Packs and rules: `kotlin`

### kotlin

Kotlin projects and Gradle Kotlin DSL builds.

Detected by: a file matching `**/build.gradle.kts`, `**/settings.gradle.kts`; or a source file of the project's own matching `**/*.kt`.

No rules of its own.

### kotlin/gradle

Gradle builds written in the Kotlin DSL.

Detected by: a file matching `**/build.gradle.kts`, `**/settings.gradle.kts`.
Child of `kotlin`: active only where it is.

No rules of its own.

| Capability (§20) | Description | Delivered | Relevant when | Active when |
|---|---|---|---|---|
| `kotlin/gradle/build-diagnose` | Finding why a Gradle build fails or resolves the wrong library version, from Gradle's own reports. | on-demand | The task is to fix a failing Gradle build or build script.<br>The task is to find why a JVM program gets the wrong version of a library, such as a NoSuchMethodError or a ClassNotFoundException. | wherever its pack is |

### kotlin/mcp

Model Context Protocol servers written with the Kotlin MCP SDK.

Detected by: a line matching `io\.modelcontextprotocol(:\|\.)kotlin-sdk` in a file matching `**/build.gradle.kts`, `**/build.gradle`; or a line matching `io\.modelcontextprotocol(:\|\.)kotlin-sdk` in a file matching `**/libs.versions.toml`.
Child of `kotlin`: active only where it is.

No rules of its own.

| Capability (§20) | Description | Delivered | Relevant when | Active when |
|---|---|---|---|---|
| `kotlin/mcp/inspector-probe` | Calling a Kotlin MCP server's tools through the MCP Inspector's command line to read their real responses. | on-demand | The task adds or changes a tool, resource or prompt of an MCP server.<br>The task is to fix or check what an MCP server's tool returns. | wherever its pack is |
