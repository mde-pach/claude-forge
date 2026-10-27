### secrets

Active in every repository (`detect: {always: true}`, §5.1). Rules are `check` lint targets.

| Rule | Targets |
|---|---|
| `secrets/gitleaks` | `forge_gitleaks`: pinned gitleaks (§13.5) `dir` on copies of the changed files (every tracked file with `--all`), with Forge's config only (`-c`: gitleaks' default rules without `generic-api-key`, which decides on identifier names and entropy, plus LangSmith `lsv2_(pt\|sk)_…` and Pinecone `pcsk_…` keys), `-i` to an empty directory (`.gitleaksignore` does not apply), `--ignore-gitleaks-allow`, `--redact`; findings `path:line rule: description`, on added lines |
| `secrets/no-credential-files` | `forge_credential_files`, when such a file or an `.npmrc` changed: tracked or added files matching `*.p12`, `*.pfx`, `*.jks`, `*.keystore`, `*.key`, `id_rsa`, `id_ecdsa`, `id_ed25519`, `.pypirc`, `.netrc`, `.git-credentials` (line 0); `.npmrc` lines setting `_authToken`, `_auth` or `_password` to a literal value (`${VAR}` is fine) |
| `secrets/env-not-committed` | `forge_env_files`, when an `@env-files` or `@env-template` file or a `.gitignore` changed: tracked or added `@env-files` (`.env`, `.env.*`, not the templates) at line 0; when a template or a `.gitignore` changed (or with `--all`), each template's directory whose `.env` git would not ignore (`git check-ignore --no-index`) |

context.md: credentials come from the environment or a secret store; a token or key found in a file, a log or a tool's output is never used to call an API or a CLI (`secrets/no-credential-reuse`); what gitleaks and the credential-file check reject.
