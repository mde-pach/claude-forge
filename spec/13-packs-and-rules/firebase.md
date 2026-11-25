<!-- Generated from the built-in packs by crates/forge-packs/tests/spec_tables.rs; do not edit. Regenerate: FORGE_BLESS=1 cargo test -p forge-packs --test spec_tables -->

## 13. Packs and rules: `firebase`

### firebase

Firebase projects (firebase.json, the Firebase SDKs and the Firebase CLI).

Detected by: a dependency on one of `firebase` (npm), `firebase-admin` (npm), `firebase-functions` (npm), `firebase-tools` (npm), `firebase-admin` (PyPI), `firebase-functions` (PyPI); or a file matching `firebase.json`, `.firebaserc`, `**/firebase.json`.

No rules of its own.

| Capability (§20) | Description | Delivered | Relevant when | Active when |
|---|---|---|---|---|
| `firebase/emulators` | Reading a Firebase project's active project and app config with the CLI and running the app against the local emulators. | on-demand | The task sets up, configures or connects an app to a Firebase project.<br>The task needs Firebase functions, auth, storage or database behaviour tried locally. | wherever its pack is |
| `firebase/remote-config-get` | Reading a Firebase project's live Remote Config template, its parameters and conditions, and its version history. | on-demand | The task adds, changes or reads a feature flag or Remote Config parameter. | wherever its pack is |

### firebase/firestore

Cloud Firestore databases, their security rules and indexes.

Detected by: a file matching `**/firestore.rules`, `**/firestore.indexes.json`; or a line matching `^\s*"firestore"\s*:` in a file matching `**/firebase.json`; or a line matching `from\s+["'](firebase/firestore\|firebase-admin/firestore\|@google-cloud/firestore)["']` in a file matching `**/*.ts`, `**/*.tsx`, `**/*.js`, `**/*.mjs`; or a line matching `^\s*from\s+firebase_admin\s+import\s+.*\bfirestore\b` in a file matching `**/*.py`.
Child of `firebase`: active only where it is.

No rules of its own.

| Capability (§20) | Description | Delivered | Relevant when | Active when |
|---|---|---|---|---|
| `firebase/firestore/database-inspect` | Listing a Firebase project's Firestore databases and reading each one's edition, mode and location. | on-demand | The task adds or changes Firestore data models, queries, indexes or database setup. | wherever its pack is |
| `firebase/firestore/rules-test` | Testing Firestore security rules on the emulator by reading and writing as different users. | on-demand | The task creates or changes Firestore security rules.<br>The task changes who may read or write which Firestore documents. | wherever its pack is |
