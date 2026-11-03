<!-- Generated from the built-in packs by crates/forge-packs/tests/spec_tables.rs; do not edit. Regenerate: FORGE_BLESS=1 cargo test -p forge-packs --test spec_tables -->

## 13. Packs and rules: `secrets`

### secrets

Credentials stay out of the repository, whatever the language.

Detected by: every repository.

Context (in `.claude/rules/forge-*.md`, §7):

- Secrets. Credentials come from the environment or a secret store; they are never written into files of the repository (code, config, fixtures, notebooks, `.env.example`).
- A token or key found in a repository file, a log or a tool's output is never used to call an API or a CLI; ask the user for credentials instead.
- A detected secret is removed, never allowlisted: no inline `gitleaks:allow` comments, `.gitleaksignore` entries or `.gitleaks.toml` allowlists.
- Keystores, private keys and auth files (`*.p12`, `*.pfx`, `*.jks`, `*.key`, `id_rsa`, `.pypirc`, `.netrc`, `.git-credentials`) are not committed; `.npmrc` tokens are `${NPM_TOKEN}`-style references.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `secrets/env-not-committed` | No dotenv file (file role @env-files: .env, .env.*) is tracked or added; templates (@env-template: .env.example, .env.sample, .env.template, .env.dist) are fine, and the .env next to a template is git-ignored. | any | check lint: `forge_env_files` (when `@env-files`, `@env-template` or `**/.gitignore` changes) |
| `secrets/gitleaks` | Added lines hold no secret gitleaks recognises (its provider rules, without generic-api-key, plus LangSmith and Pinecone keys), in any file; inline gitleaks:allow, .gitleaksignore and a project .gitleaks.toml do not apply. | any | check lint: `forge_gitleaks` |
| `secrets/no-credential-files` | No keystore, private key or auth file is committed (*.p12, *.pfx, *.jks, *.keystore, *.key, id_rsa, id_ecdsa, id_ed25519, .pypirc, .netrc, .git-credentials), and no .npmrc line sets _authToken, _auth or _password to a literal value (a ${VAR} reference is fine). | any | check lint: `forge_credential_files` (when `**/*.p12`, `**/*.pfx`, `**/*.jks`, `**/*.keystore`, `**/*.key`, `**/id_rsa`, `**/id_ecdsa`, `**/id_ed25519`, `**/.pypirc`, `**/.netrc`, `**/.git-credentials` or `**/.npmrc` changes) |
