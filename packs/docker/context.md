Docker. Images are multi-stage: build tools stay in a builder stage, the final stage copies only what runs.
The final stage runs as a non-root `USER`.
Dependencies install from the lock file (`npm ci`, `bun install --frozen-lockfile`, `uv sync --locked`), with cache mounts (`RUN --mount=type=cache,target=...`).
Secrets reach a build through `RUN --mount=type=secret,id=...`, never `ARG` or `ENV`.
Base images are pinned to a version tag (or a digest); `.dockerignore` excludes `.env` files.
Claude Code's sandbox blocks docker unless the user lists it in `sandbox.excludedCommands`; docker commands then run outside the sandbox.
