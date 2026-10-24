# syntax=docker/dockerfile:1.7
# forge-exp/inngest-js:v1 -- see harness/IMAGES_TS.md (TypeScript section).
#
# Build (from experiment/):
#   DOCKER_BUILDKIT=1 docker build -f harness/images/inngest-js.Dockerfile \
#     --secret id=ca,src=/root/.ccr/ca-bundle.crt -t forge-exp/inngest-js:v1 harness/images
#
# Only adds the exact pnpm versions named by `packageManager` in the candidate commits
# (corepack would otherwise try to download them, and trials are offline). Dependencies
# are NOT baked in: they come from the host pnpm store mounted read-only at /cache/pnpm.
FROM forge-exp/base:v1

USER dev
# pnpm version seen in inngest-js candidate parents (main + pilot windows): pnpm@10.12.1 (with
#   the +sha512 integrity suffix, checked by corepack at download). The base default (10.34.7) is restored last,
#   so a directory without `packageManager` behaves exactly as in the base image.
RUN --mount=type=secret,id=ca,target=/run/secrets/ca,uid=1000 \
    export NODE_EXTRA_CA_CERTS=/run/secrets/ca \
 && corepack install -g "pnpm@10.12.1+sha512.f0dda8580f0ee9481c5c79a1d927b9164f2c478e90992ad268bbb2465a736984391d6333d2c327913578b2804af33474ca554ba29c04a8b13060a717675ae3ac" \
 && corepack install -g pnpm@10.34.7 \
 && cat /opt/corepack/lastKnownGood.json && ls /opt/corepack/v1/pnpm

ENV npm_config_update_notifier=false \
    COREPACK_ENABLE_NETWORK=0

WORKDIR /work
CMD ["sleep", "infinity"]
