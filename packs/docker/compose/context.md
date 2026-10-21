Docker Compose. `compose.yaml` holds the shared setup; `compose.override.yaml` (loaded automatically) holds local changes.
Services have a `healthcheck`, and dependents wait with `depends_on: {db: {condition: service_healthy}}`.
`develop.watch` syncs or rebuilds on file changes (`docker compose watch`); `profiles` keep optional services off by default.
Secrets come from `secrets:` (files mounted under /run/secrets) or `${VAR}` references, never inline values; `.env` is not committed.
Published database ports bind to `127.0.0.1` (`"127.0.0.1:5432:5432"`): Docker-published ports bypass ufw and firewalld.
docker compose runs outside Claude Code's sandbox, so these rules are a real boundary.
