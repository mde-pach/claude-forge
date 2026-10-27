### docker

Detected by `**/Dockerfile`, `**/Dockerfile.*`, `**/*.Dockerfile`, `**/Containerfile` or a compose file (a repository may use Compose only for a local database). Rules are `check` lint targets on the Dockerfiles of the change (every tracked one with `--all`).

| Rule | Targets |
|---|---|
| `docker/pin-base-image` | `forge_hadolint DL3006 DL3007` (§13.3) |
| `docker/copy-not-add` | `forge_hadolint DL3020` |
| `docker/exec-form` | `forge_hadolint DL3025` |
| `docker/workdir` | `forge_hadolint DL3045` |
| `docker/pipefail` | `forge_hadolint DL4006` (DL3008, pinned apt versions, is not enforced: they disappear from mirrors) |
| `docker/non-root-user` | `forge_docker_nonroot`: the final stage has a `USER` other than `root` / `0`, set in it or inherited from the named stage it builds on; base images under `cgr.dev/` or `dhi.io/`, or named `*nonroot*`, `*unprivileged*`, `*rootless*`, are exempt (hadolint's DL3002 misses a Dockerfile with no `USER`) |
| `docker/no-secret-arg-env` | `forge_docker_secrets`: no `ARG` / `ENV` named like `*PASSWORD*`, `*SECRET*`, `*TOKEN*`, `*API_KEY*`, `*PRIVATE_KEY*`, `*CREDENTIALS*` (`*_FILE` is fine); use `RUN --mount=type=secret` |
| `docker/locked-install` | `forge_docker_locked`: in `RUN` (continuations joined, commands split on `&&`, `\|\|`, `;`): no `npm install` / `npm i` (except `-g`), `bun install` with `--frozen-lockfile` and no `bun add`, `uv sync` with `--locked` or `--frozen` and no `uv add`, `pnpm install` with `--frozen-lockfile`, `yarn` with `--immutable` or `--frozen-lockfile` |
| `docker/dockerignore` | `forge_docker_ignore`: each Dockerfile has a `.dockerignore` (`<Dockerfile>.dockerignore`, its directory's, or the root's) that excludes `.env` files |
| `docker/hadolint-correctness` | `forge_hadolint DL3000 DL3004 DL3014 DL3022 DL3023 DL3024 DL3027 DL3044 DL3061 DL3063 DL3067 DL4000 DL4003 DL4004` (relative `WORKDIR`, `sudo`, `apt-get install` without `-y`, stage references and aliases, `apt`, self-referencing `ENV`, instruction order, `COPY --from` to `/`, `MAINTAINER`, several `CMD` / `ENTRYPOINT`) |
| `docker/image-size` | advisory (§10.7): `forge_advise forge_hadolint DL3009 DL3015 DL3042 DL3059` (apt lists, `--no-install-recommends`, pip cache, consecutive `RUN`) |
| `docker/deps-before-copy` | advisory (§10.7): `forge_advise forge_docker_copy_order`: in a stage, a `RUN` installing dependencies (`npm ci` / `install`, `bun install`, `uv sync`, `pnpm install`, `yarn`, `pip install -r`) after a `COPY` / `ADD` of `.` or `./` |

Structural findings (`non-root-user`, `dockerignore`) are reported at line 0, so they count whenever the Dockerfile changed (removing a `USER` adds no line). context.md: multi-stage builds, a non-root final stage, lock-file installs with cache mounts, build secrets, pinned base images; the sandbox blocks docker unless the user lists it in `sandbox.excludedCommands`. Forge does not write that setting; `forge status` prints the line to add when the pack is active.

### docker/compose

Detected by `**/compose*.y*ml` or `**/docker-compose*.y*ml`. ast-grep rules use `scope.language: yaml` on the compose files.

| Rule | Targets |
|---|---|
| `docker/compose/config-valid` | `check` lint: `docker compose config --quiet --no-interpolate --no-env-resolution` on the root compose file (no `.env` is read) |
| `docker/compose/no-version-key` | `ast-grep` forbid: a top-level `version` key |
| `docker/compose/no-docker-sock` | `ast-grep` forbid: a scalar naming `/run/docker.sock` or `/var/run/docker.sock` |
| `docker/compose/no-host-root-mounts` | `ast-grep` forbid: under `volumes`, a short-syntax source or a long-syntax `source` of `/`, `/etc`, `/root`, `/home`, `~`, `$HOME` |
| `docker/compose/no-privileged` | `ast-grep` forbid: `privileged: true`; `network_mode`, `pid`, `ipc`, `userns_mode` or `uts` set to `host`; `cap_add` with `ALL` or `SYS_ADMIN`; a `security_opt` entry with `unconfined` |
| `docker/compose/datastore-loopback` | `ast-grep` forbid: a short-syntax `ports` entry publishing 5432, 3306, 6379, 27017, 9200, 5672, 11211 or 1433 that does not start with `127.0.0.1:` (or `localhost:`, `[::1]:`); Docker-published ports bypass ufw and firewalld. Long syntax is not checked |
| `docker/compose/no-inline-secrets` | `ast-grep` forbid: under `environment` or `args`, a secret-like key (map form) or `KEY=value` entry (list form) whose value is neither empty nor a `${VAR}` reference; `*_FILE` keys are fine |
| `docker/compose/healthy-depends` | `ast-grep` forbid: `depends_on` as a list (block or flow); the long form with `condition:` is required |
| `docker/compose/pinned-images` | `ast-grep` forbid: `image:` with `:latest` or no tag or digest, unless the service also has `build:`; `${VAR}` images are not checked |
| `docker/compose/no-links` | `ast-grep` forbid: `links` / `external_links` in a service |
| `docker/compose/env-not-committed` | deprecated, replaced by `secrets/env-not-committed` (every repository, not only Compose projects) |

context.md: `compose.yaml` + `compose.override.yaml`, healthchecks with `condition: service_healthy`, `develop.watch`, profiles, secrets, loopback-bound database ports, and that docker compose runs outside the sandbox, which makes these rules a real boundary.
