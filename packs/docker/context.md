Docker. Multi-stage builds keep build tools out of the final stage; the checks require the final stage to run as a non-root `USER`.
`RUN` installs from the lock file: `npm ci`, `bun install --frozen-lockfile`, `uv sync --locked`, `pnpm install --frozen-lockfile`, `yarn --immutable`; `bun add` and `uv add` are rejected. Cache mounts (`RUN --mount=type=cache,target=...`) speed them up.
`ARG` and `ENV` names like `*TOKEN*`, `*SECRET*`, `*PASSWORD*`, `*API_KEY*` are rejected (`*_FILE` is fine); build secrets use `RUN --mount=type=secret,id=...`.
Base images name a version tag or a digest (not `latest`); each Dockerfile has a `.dockerignore` that excludes `.env` files.
Claude Code's sandbox blocks docker unless the user lists it in `sandbox.excludedCommands`; docker commands then run outside the sandbox.
