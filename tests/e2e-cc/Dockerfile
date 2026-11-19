# The CI stand-in for the local bench image: a plain-scripts Python project (no packaging file)
# committed at /app, with git, curl and Python. No Claude Code and no Forge: run.sh installs
# both, as it does on the bench image.
FROM python:3.13-slim-bookworm
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl git \
    && rm -rf /var/lib/apt/lists/*
COPY fixture/ /app/
WORKDIR /app
RUN git init -q -b main \
    && git add -A \
    && git -c user.name=fixture -c user.email=fixture@example.invalid commit -qm "fixture baseline"
