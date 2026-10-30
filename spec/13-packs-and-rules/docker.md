<!-- Generated from the built-in packs by crates/forge-packs/tests/spec_tables.rs; do not edit. Regenerate: FORGE_BLESS=1 cargo test -p forge-packs --test spec_tables -->

## 13. Packs and rules: `docker`

### docker

Docker images and Compose files.

Detected by: a file matching `**/Dockerfile`, `**/Dockerfile.*`, `**/*.Dockerfile`, `**/Containerfile`, `**/compose*.yml`, `**/compose*.yaml`, `**/docker-compose*.yml`, `**/docker-compose*.yaml`.

Context (in `.claude/rules/forge-*.md`, §7):

- (when a file matches `**/Dockerfile`, `**/Dockerfile.*`, `**/*.Dockerfile`, `**/Containerfile`) Docker. Multi-stage builds keep build tools out of the final stage, and the final stage runs as a non-root `USER`.
- (when a file matches `**/Dockerfile`, `**/Dockerfile.*`, `**/*.Dockerfile`, `**/Containerfile`) `RUN` installs from the lock file: `npm ci`, `bun install --frozen-lockfile`, `uv sync --locked`, `pnpm install --frozen-lockfile`, `yarn --immutable`; `bun add` and `uv add` are rejected. Cache mounts (`RUN --mount=type=cache,target=...`) speed them up.
- (when a file matches `**/Dockerfile`, `**/Dockerfile.*`, `**/*.Dockerfile`, `**/Containerfile`) `ARG` and `ENV` names like `*TOKEN*`, `*SECRET*`, `*PASSWORD*`, `*API_KEY*` are rejected (`*_FILE` is fine); build secrets use `RUN --mount=type=secret,id=...`.
- (when a file matches `**/Dockerfile`, `**/Dockerfile.*`, `**/*.Dockerfile`, `**/Containerfile`) Base images name a version tag or a digest (not `latest`); each Dockerfile has a `.dockerignore` that excludes `.env` files.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `docker/copy-not-add` | COPY, not ADD, for files and folders (hadolint DL3020). | any: `@dockerfile` | check lint: `forge_hadolint DL3020` (when `@dockerfile` changes) |
| `docker/deps-before-copy` | In a build stage, copy the manifest and lock file and install dependencies before COPY . ., so a code change does not rebuild the dependency layer. | any: `@dockerfile` | check lint: `forge_advise forge_docker_copy_order` (when `@dockerfile` changes)<br>advice, never blocks |
| `docker/dockerignore` | Each Dockerfile has a .dockerignore (Dockerfile.dockerignore next to it, one in its directory, or one at the root) that excludes .env files from the build context. | any: `@dockerfile` | check lint: `forge_docker_ignore` (when `@dockerfile`, `**/.dockerignore` or `**/*.dockerignore` changes) |
| `docker/exec-form` | CMD and ENTRYPOINT use the JSON (exec) form, so signals reach the process (hadolint DL3025). | any: `@dockerfile` | check lint: `forge_hadolint DL3025` (when `@dockerfile` changes) |
| `docker/hadolint-correctness` | Dockerfiles avoid build-breaking and wrong-behaviour mistakes hadolint finds: a relative WORKDIR (DL3000), sudo (DL3004), apt-get install without -y (DL3014), COPY --from a missing or own stage or an alias used twice (DL3022, DL3023, DL3024), apt instead of apt-get (DL3027), an ENV referencing itself (DL3044), instructions before FROM (DL3061), DL3063, COPY --from a stage to / (DL3067), MAINTAINER (DL4000), several CMD or ENTRYPOINT (DL4003, DL4004). | any: `**/Dockerfile`, `**/Dockerfile.*`, `**/*.Dockerfile`, `**/Containerfile` | check lint: a shell script calling `forge_hadolint` (when `**/Dockerfile`, `**/Dockerfile.*`, `**/*.Dockerfile` or `**/Containerfile` changes) |
| `docker/image-size` | Smaller images (hadolint DL3009: delete the apt lists; DL3015: apt-get install --no-install-recommends; DL3042: pip install --no-cache-dir or a cache mount; DL3059: consecutive RUN instructions merged). | any: `**/Dockerfile`, `**/Dockerfile.*`, `**/*.Dockerfile`, `**/Containerfile` | check lint: `forge_advise forge_hadolint DL3009 DL3015 DL3042 DL3059` (when `**/Dockerfile`, `**/Dockerfile.*`, `**/*.Dockerfile` or `**/Containerfile` changes)<br>advice, never blocks |
| `docker/locked-install` | Images install dependencies from the lock file (npm ci, bun install --frozen-lockfile, uv sync --locked, pnpm install --frozen-lockfile, yarn --immutable) and never add packages. | any: `@dockerfile` | check lint: `forge_docker_locked` (when `@dockerfile` changes) |
| `docker/no-secret-arg-env` | No ARG or ENV named like a secret (PASSWORD, SECRET, TOKEN, API_KEY, PRIVATE_KEY, CREDENTIALS); they stay in the image history. *_FILE names are fine; use RUN --mount=type=secret. | any: `@dockerfile` | check lint: `forge_docker_secrets` (when `@dockerfile` changes) |
| `docker/non-root-user` | The final stage runs as a non-root USER, set in it or inherited from the named stage it builds on (images that run non-root by default, cgr.dev/, dhi.io/ or *nonroot*, are exempt). | any: `@dockerfile` | check lint: `forge_docker_nonroot` (when `@dockerfile` changes) |
| `docker/pin-base-image` | Base images name a version tag (or a digest), never latest or nothing (hadolint DL3006, DL3007). | any: `@dockerfile` | check lint: `forge_hadolint DL3006 DL3007` (when `@dockerfile` changes) |
| `docker/pipefail` | A RUN with a pipe sets SHELL with -o pipefail first, so a failing left side fails the build (hadolint DL4006). | any: `@dockerfile` | check lint: `forge_hadolint DL4006` (when `@dockerfile` changes) |
| `docker/workdir` | COPY to a relative destination only after WORKDIR (hadolint DL3045). | any: `@dockerfile` | check lint: `forge_hadolint DL3045` (when `@dockerfile` changes) |

### docker/compose

Docker Compose files.

Detected by: a file matching `**/compose*.yml`, `**/compose*.yaml`, `**/docker-compose*.yml`, `**/docker-compose*.yaml`.
Child of `docker`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- Docker Compose. The base compose file (`compose.yaml`, or `docker-compose.yml`) holds the shared setup; its override file (`compose.override.yaml`, or `docker-compose.override.yml`, loaded automatically) holds local changes.
- `depends_on` uses the long form with a `condition` (the bare list form is rejected); `condition: service_healthy` needs a `healthcheck` on that service.
- `develop.watch` syncs or rebuilds on file changes (`docker compose watch`); `profiles` keep optional services off by default.
- Variables named like `*PASSWORD*`, `*SECRET*`, `*TOKEN*`, `*API_KEY*` under `environment` or build `args` hold a `${VAR}` reference or nothing (`*_FILE` is fine); a committed `.env` is rejected.
- Published ports 5432, 3306, 6379, 27017, 9200, 5672, 11211 and 1433 bind to `127.0.0.1` (`"127.0.0.1:5432:5432"`): Docker-published ports bypass ufw and firewalld.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `docker/compose/config-valid` | The root compose file is valid (docker compose config --quiet --no-interpolate --no-env-resolution: variables are not substituted and env_file files are not read; Compose still parses the project .env). | any: `@compose` | check lint: a shell script calling `forge_do docker validate` (when `@compose` or `**/.env.example` changes) |
| `docker/compose/datastore-loopback` | Published database and broker ports (5432, 3306, 6379, 27017, 9200, 5672, 11211, 1433) bind to 127.0.0.1; Docker-published ports bypass ufw and firewalld. | yaml: `@compose` | ast-grep forbid |
| `docker/compose/env-not-committed` | No .env file is committed (.env.example, .env.sample and .env.template are fine). | any | check lint: `! git ls-files \| grep -E '(^\|/)\.env(\..+)?$' \| grep -vE '(^\|/)\.env\.(example\|sample\|template)$'` (when `**/.env` or `**/.env.*` changes)<br>deprecated: replaced by `secrets/env-not-committed` |
| `docker/compose/healthy-depends` | depends_on uses the long form with a condition (service_healthy for databases), never the bare list, which only waits for the container to start. | yaml: `@compose` | ast-grep forbid |
| `docker/compose/no-docker-sock` | No service mounts the Docker socket; it gives the container root on the host. | yaml: `@compose` | ast-grep forbid |
| `docker/compose/no-host-root-mounts` | No service mounts /, /etc, /root, /home or the home directory from the host. | yaml: `@compose` | ast-grep forbid |
| `docker/compose/no-inline-secrets` | Secret-like variables under environment or build args (PASSWORD, SECRET, TOKEN, API_KEY, PRIVATE_KEY, CREDENTIALS) hold a ${VAR} reference or nothing, never a value; *_FILE variables are fine. | yaml: `@compose` | ast-grep forbid |
| `docker/compose/no-links` | No links: or external_links: in services; services reach each other by name on a shared network. | yaml: `**/compose*.yml`, `**/compose*.yaml`, `**/docker-compose*.yml`, `**/docker-compose*.yaml` | ast-grep forbid |
| `docker/compose/no-privileged` | No privileged service, host namespaces (network_mode, pid, ipc, userns_mode, uts: host), ALL or SYS_ADMIN capabilities, or unconfined security options. | yaml: `@compose` | ast-grep forbid |
| `docker/compose/no-version-key` | No top-level version key; Compose ignores it and warns. | yaml: `@compose` | ast-grep forbid |
| `docker/compose/pinned-images` | Images name a version tag (or a digest), never latest or nothing, except in a service that builds its own image. | yaml: `@compose` | ast-grep forbid |
