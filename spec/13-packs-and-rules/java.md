<!-- Generated from the built-in packs by crates/forge-packs/tests/spec_tables.rs; do not edit. Regenerate: FORGE_BLESS=1 cargo test -p forge-packs --test spec_tables -->

## 13. Packs and rules: `java`

### java

Java and other JVM projects built with Maven or Gradle.

Detected by: a file matching `**/pom.xml`, `**/build.gradle`, `**/build.gradle.kts`, `**/settings.gradle`, `**/settings.gradle.kts`; or a source file of the project's own matching `**/*.java`.

No rules of its own.

### java/kafka-streams

Kafka Streams applications.

Detected by: a line matching `<artifactId>kafka-streams</artifactId>` in a file matching `**/pom.xml`; or a line matching `org\.apache\.kafka[:"'].*kafka-streams\|kafka-streams["']?\s*[:=]` in a file matching `**/build.gradle`, `**/build.gradle.kts`, `**/libs.versions.toml`.
Child of `java`: active only where it is.

No rules of its own.

| Capability (§20) | Description | Delivered | Relevant when | Active when |
|---|---|---|---|---|
| `java/kafka-streams/run-streams-app` | Running a Kafka Streams application against a local broker and watching it reach RUNNING and process records. | on-demand | The task builds or changes a Kafka Streams topology or its configuration.<br>The task is to find why a stream processing application is stuck, fails or stops processing. | wherever its pack is |
