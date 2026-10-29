<!-- Generated from the built-in packs by crates/forge-packs/tests/spec_tables.rs; do not edit. Regenerate: FORGE_BLESS=1 cargo test -p forge-packs --test spec_tables -->

## 13. Packs and rules: `openapi`

### openapi

OpenAPI documents, whatever the language that serves them.

Detected by: a file matching `**/openapi.yaml`, `**/openapi.yml`, `**/openapi.json`, `redocly.yaml`, `.spectral.yaml`, `.spectral.yml`, `.spectral.json`.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `openapi/config-ratchet` | The OpenAPI linter config is not loosened: in redocly.yaml or .spectral.yaml a rule lowered (error > warn > info / hint > off) or removed, an extends entry removed or an overrides entry added, and new entries in .redocly.lint-ignore.yaml, are reported; loosening the linter is the user's decision: ask them. | any: `redocly.yaml`, `.redocly.yaml`, `.spectral.yaml`, `.spectral.yml`, `.redocly.lint-ignore.yaml` | check lint: `forge_openapi_ratchet` (when `redocly.yaml`, `.redocly.yaml`, `.spectral.yaml`, `.spectral.yml` or `.redocly.lint-ignore.yaml` changes) |
| `openapi/lint` | OpenAPI documents pass the project's linter on the lines the change adds: the pinned Redocly CLI with redocly.yaml, the pinned Spectral CLI with .spectral.yaml (.yml, .json), both when both are configured, else Redocly's recommended rules; errors count. | any: `**/*.yaml`, `**/*.yml`, `**/*.json` | check lint: `forge_openapi_lint` (when `**/*.yaml`, `**/*.yml` or `**/*.json` changes) |
| `openapi/no-breaking` | A changed OpenAPI document has no breaking change against its base version (oasdiff breaking, level ERR: removed paths and fields, new required parameters, narrowed types), $refs to other files included; a breaking change is the user's decision: ask them. | any: `**/*.yaml`, `**/*.yml`, `**/*.json` | check lint: `forge_once forge_oasdiff` (when `**/*.yaml`, `**/*.yml` or `**/*.json` changes) |
