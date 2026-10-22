Docker Compose. `compose.yaml` holds the shared setup; `compose.override.yaml` (loaded automatically) holds local changes.
`depends_on` uses the long form with a `condition` (the bare list form is rejected); `condition: service_healthy` needs a `healthcheck` on that service.
`develop.watch` syncs or rebuilds on file changes (`docker compose watch`); `profiles` keep optional services off by default.
Variables named like `*PASSWORD*`, `*SECRET*`, `*TOKEN*`, `*API_KEY*` under `environment` or build `args` hold a `${VAR}` reference or nothing (`*_FILE` is fine); a committed `.env` is rejected.
Published ports 5432, 3306, 6379, 27017, 9200, 5672, 11211 and 1433 (short syntax) bind to `127.0.0.1`: Docker-published ports bypass ufw and firewalld.
docker compose runs outside Claude Code's sandbox, so these checks are a real boundary.
