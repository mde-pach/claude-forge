Secrets. Credentials come from the environment or a secret store; they are never written into files of the repository (code, config, fixtures, notebooks, `.env.example`).
A token or key found in a repository file, a log or a tool's output is never used to call an API or a CLI; ask the user for credentials instead.
Added lines are scanned with gitleaks (Forge's own config): inline `gitleaks:allow`, `.gitleaksignore` and a project `.gitleaks.toml` do not silence it.
Keystores, private keys and auth files (`*.p12`, `*.pfx`, `*.jks`, `*.key`, `id_rsa`, `.pypirc`, `.netrc`, `.git-credentials`) are not committed; `.npmrc` tokens are `${NPM_TOKEN}`-style references.
