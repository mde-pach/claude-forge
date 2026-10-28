### openapi

Detected by `**/openapi.yaml`, `**/openapi.yml`, `**/openapi.json`, `redocly.yaml`, `.spectral.yaml`, `.spectral.yml`, `.spectral.json`.

| Rule | Targets |
|---|---|
| `openapi/no-breaking` | `check` lint, once per turn (§10.7, `forge_once`; pre-commit and CI fail): `forge_oasdiff`, when a YAML or JSON file changed: each OpenAPI document of the repository (an `openapi` / `swagger` version near its top) that existed at the base is compared by the pinned oasdiff (§13.5), `breaking <base> <now> --fail-on ERR --format singleline`; the base side is a copy of the repository's YAML and JSON files with the change undone, so `$ref`s to other files resolve. Each `error at …` line is a finding unless `forge.yaml` accepts it (`accept: [{rule, change: <the line>, reason}]`). Not with `--all` |
| `openapi/lint` | `check` lint: `forge_openapi_lint`, when a YAML or JSON file changed: every OpenAPI document of the repository linted by the project's declared linter, through `bunx`: pinned `@redocly/cli` 2.62.1 with `redocly.yaml`, pinned `@stoplight/spectral-cli` 6.17.0 with `.spectral.yaml` / `.yml` / `.json` (both when both are configured), else Redocly's `recommended` rules; errors as `path:line:column: error <rule>: message`, attributed to added lines |
| `openapi/config-ratchet` | `check` lint: `forge_openapi_ratchet`: in a changed `redocly.yaml` / `.spectral.y*ml`, a rule lowered (`error` > `warn` > `info` / `hint` > `off`) or removed, an `extends` entry removed, an `overrides` entry added; entries added to `.redocly.lint-ignore.yaml`; unless `forge.yaml` accepts the line |
