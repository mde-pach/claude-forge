<!-- Generated from the built-in packs by crates/forge-packs/tests/spec_tables.rs; do not edit. Regenerate: FORGE_BLESS=1 cargo test -p forge-packs --test spec_tables -->

## 13. Packs and rules: `dart`

### dart

Dart packages and applications.

Detected by: a file matching `**/pubspec.yaml`.

No rules of its own.

| Capability (§20) | Description | Delivered | Relevant when | Active when |
|---|---|---|---|---|
| `dart/coverage-gaps` | Finding the Dart or Flutter code no test executes, from an lcov coverage report. | on-demand | The task is to add tests for existing code or find what the tests leave untested. | wherever its pack is |

### dart/flutter

Flutter applications and packages.

Detected by: a line matching `^\s+sdk:\s*["']?flutter["']?\s*(#.*)?$` in a file matching `**/pubspec.yaml`.
Child of `dart`: active only where it is.

No rules of its own.

| Capability (§20) | Description | Delivered | Relevant when | Active when |
|---|---|---|---|---|
| `dart/flutter/gen-l10n` | Regenerating a Flutter app's localizations from its ARB files and reading the generated accessors. | on-demand | The task adds or changes text shown to users of a Flutter app.<br>The task adds or changes a translation or a supported language of a Flutter app. | wherever its pack is |
| `dart/flutter/integration-drive` | Driving a Flutter app through its widget tree in an integration test to reproduce a UI behaviour. | on-demand | The task changes how a Flutter screen behaves when a person taps, types or scrolls.<br>The task is to write an integration test or reproduce a bug in a Flutter app's user interface. | wherever its pack is |
| `dart/flutter/size-analysis` | Measuring what makes a Flutter build large, package by package, with --analyze-size. | on-demand | The task is to reduce the size of an app's build or download.<br>The task needs an app's build size measured, explained or compared. | wherever its pack is |
