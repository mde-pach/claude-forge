# syntax=docker/dockerfile:1.7
# forge-exp/better-auth:v1 -- see harness/IMAGES_TS.md (TypeScript section).
#
# Build (from experiment/):
#   DOCKER_BUILDKIT=1 docker build -f harness/images/better-auth.Dockerfile \
#     --secret id=ca,src=/root/.ccr/ca-bundle.crt -t forge-exp/better-auth:v1 harness/images
#
# Only adds the exact pnpm versions named by `packageManager` in the candidate commits
# (corepack would otherwise try to download them, and trials are offline). Dependencies
# are NOT baked in: they come from the host pnpm store mounted read-only at /cache/pnpm.
FROM forge-exp/base:v1

USER dev
# pnpm versions seen in better-auth candidate parents (main + pilot windows):
#   pnpm@10.30.2, pnpm@11.0.3, pnpm@11.1.1. The base default (10.34.7) is restored last,
#   so a directory without `packageManager` behaves exactly as in the base image.
RUN --mount=type=secret,id=ca,target=/run/secrets/ca,uid=1000 \
    export NODE_EXTRA_CA_CERTS=/run/secrets/ca \
 && for v in 10.30.2 11.0.3 11.1.1; do corepack install -g "pnpm@$v"; done \
 && corepack install -g pnpm@10.34.7 \
 && cat /opt/corepack/lastKnownGood.json && ls /opt/corepack/v1/pnpm

ENV npm_config_update_notifier=false \
    COREPACK_ENABLE_NETWORK=0

WORKDIR /work
CMD ["sleep", "infinity"]
