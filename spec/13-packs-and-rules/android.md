<!-- Generated from the built-in packs by crates/forge-packs/tests/spec_tables.rs; do not edit. Regenerate: FORGE_BLESS=1 cargo test -p forge-packs --test spec_tables -->

## 13. Packs and rules: `android`

### android

Android applications and libraries built with Gradle and the Android Gradle plugin.

Detected by: a file matching `**/src/main/AndroidManifest.xml`; or a line matching `com\.android\.(application\|library)` in a file matching `**/build.gradle`, `**/build.gradle.kts`; or a line matching `id\s*=\s*"com\.android\.(application\|library)"` in a file matching `**/libs.versions.toml`.

No rules of its own.

| Capability (§20) | Description | Delivered | Relevant when | Active when |
|---|---|---|---|---|
| `android/benchmark-compare` | Comparing Android build or runtime configurations with repeated, order-balanced Macrobenchmark runs and their traces. | on-demand | The task compares the performance of two or more configurations, builds or code versions of an Android app.<br>The task chooses a default setting from measured Android performance results. | wherever its pack is |
| `android/compose-screenshot-parity` | Keeping an XML layout's look when migrating it to Jetpack Compose, by comparing screenshots before and after. | on-demand | The task migrates an Android XML layout or View to Jetpack Compose. | wherever its pack is |
| `android/run-on-device` | Building an Android variant, installing it on an emulator or device, launching it and reading its logs and screen. | on-demand | The task is to find why an app crashes or misbehaves.<br>The task needs an app's screen or behaviour checked on a device or emulator. | wherever its pack is |
