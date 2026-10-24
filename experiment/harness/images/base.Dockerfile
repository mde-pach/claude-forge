# syntax=docker/dockerfile:1.7
# forge-exp/base:v1 -- shared image for both experiment arms (see harness/IMAGES.md).
#
# Build (from experiment/):
#   DOCKER_BUILDKIT=1 docker build -f harness/images/base.Dockerfile \
#     --secret id=ca,src=/root/.ccr/ca-bundle.crt -t forge-exp/base:v1 harness/images
#
# The proxy CA is passed as a BuildKit secret: it is used for the downloads below and
# is NOT baked into the image (trial containers get it via a runtime mount if needed).
FROM debian:bookworm-slim@sha256:7ff6ba31527b9e336162fa061746c366c4b2032f4d2cff2078bea43f78efe3f8

ARG UV_VERSION=0.12.24
ARG UV_SHA256=b4dfaef47d491a7296981f8374a4595f55dbf84e8937c8ecd2983574d8bb3da6
ARG NODE_VERSION=22.23.3
ARG NODE_SHA256=df450af89261115ef9f9e3830c3eeb2cc9213b63c720b1af623cb5dcbe2e02de
ARG BUN_VERSION=1.4.3
ARG BUN_SHA256=3e730528d89775f03f87f05c06787c4df4099d0a37f3f252e7b829cb23816e18
ARG PNPM_VERSION=10.34.7
# Forge 091c0d3 (src/util.ts) pins
ARG AST_GREP_VERSION=0.45.3
ARG SQUAWK_VERSION=2.70.0
ARG ZIZMOR_VERSION=1.30.1
ARG HADOLINT_VERSION=2.15.1

ENV LANG=C.UTF-8 \
    LC_ALL=C.UTF-8 \
    DEBIAN_FRONTEND=noninteractive

RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      git curl ca-certificates bubblewrap socat ripgrep jq procps python3 xz-utils \
 && rm -rf /var/lib/apt/lists/* /var/cache/apt/archives/*

RUN groupadd -g 1000 dev && useradd -m -u 1000 -g 1000 -d /home/dev -s /bin/bash dev \
 && mkdir -p /work /cache /opt/uv-python /opt/forge-cache/uv /opt/forge-cache/bun /opt/corepack \
 && chown -R dev:dev /work /opt/uv-python /opt/forge-cache /opt/corepack

# uv (static binary)
RUN --mount=type=secret,id=ca,target=/run/secrets/ca \
    curl -fsSL --cacert /run/secrets/ca -o /tmp/uv.tgz \
      "https://github.com/astral-sh/uv/releases/download/${UV_VERSION}/uv-x86_64-unknown-linux-gnu.tar.gz" \
 && echo "${UV_SHA256}  /tmp/uv.tgz" | sha256sum -c - \
 && tar -xzf /tmp/uv.tgz -C /usr/local/bin --strip-components=1 \
 && rm /tmp/uv.tgz && uv --version

# Node 22 + corepack
RUN --mount=type=secret,id=ca,target=/run/secrets/ca \
    curl -fsSL --cacert /run/secrets/ca -o /tmp/node.txz \
      "https://nodejs.org/dist/v${NODE_VERSION}/node-v${NODE_VERSION}-linux-x64.tar.xz" \
 && echo "${NODE_SHA256}  /tmp/node.txz" | sha256sum -c - \
 && mkdir -p /opt/node && tar -xJf /tmp/node.txz -C /opt/node --strip-components=1 \
 && rm /tmp/node.txz \
 && rm -rf /opt/node/include /opt/node/share/doc /opt/node/share/man

# bun
RUN --mount=type=secret,id=ca,target=/run/secrets/ca \
    curl -fsSL --cacert /run/secrets/ca -o /tmp/bun.zip \
      "https://github.com/oven-sh/bun/releases/download/bun-v${BUN_VERSION}/bun-linux-x64.zip" \
 && echo "${BUN_SHA256}  /tmp/bun.zip" | sha256sum -c - \
 && python3 -m zipfile -e /tmp/bun.zip /tmp/bun \
 && install -m 0755 /tmp/bun/bun-linux-x64/bun /usr/local/bin/bun \
 && ln -s /usr/local/bin/bun /usr/local/bin/bunx \
 && rm -rf /tmp/bun /tmp/bun.zip && bun --version

ENV PATH=/opt/node/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
    UV_PYTHON_INSTALL_DIR=/opt/uv-python \
    UV_PYTHON_DOWNLOADS=never \
    UV_LINK_MODE=copy \
    UV_CACHE_DIR=/opt/forge-cache/uv \
    BUN_INSTALL_CACHE_DIR=/opt/forge-cache/bun \
    COREPACK_HOME=/opt/corepack \
    COREPACK_ENABLE_DOWNLOAD_PROMPT=0 \
    COREPACK_ENABLE_AUTO_PIN=0

RUN corepack enable --install-directory /opt/node/bin pnpm

USER dev
WORKDIR /home/dev

# Managed Pythons (shared, readable by dev) + pnpm via corepack.
RUN --mount=type=secret,id=ca,target=/run/secrets/ca,uid=1000 \
    SSL_CERT_FILE=/run/secrets/ca UV_NATIVE_TLS=1 UV_PYTHON_DOWNLOADS=automatic \
      uv python install 3.12 3.13 \
 && NODE_EXTRA_CA_CERTS=/run/secrets/ca corepack install -g "pnpm@${PNPM_VERSION}" \
 && pnpm --version

# Pre-warm Forge's pinned tools (ast-grep, zizmor, hadolint, squawk) through the exact
# uvx / bunx invocations Forge's hooks use, so they run offline. Parity-neutral: both arms.
RUN --mount=type=secret,id=ca,target=/run/secrets/ca,uid=1000 \
    export SSL_CERT_FILE=/run/secrets/ca UV_NATIVE_TLS=1 NODE_EXTRA_CA_CERTS=/run/secrets/ca \
 && uvx -q --from "ast-grep-cli==${AST_GREP_VERSION}" ast-grep --version \
 && uvx -q "zizmor@${ZIZMOR_VERSION}" --version \
 && uvx -q --from "hadolint-bin==${HADOLINT_VERSION}" python -c 'import shutil; print(shutil.which("hadolint"))' \
 && uvx -q --from "squawk-cli==${SQUAWK_VERSION}" squawk --version \
 && cd /tmp \
 && bunx -p "@ast-grep/cli@${AST_GREP_VERSION}" ast-grep --version \
 && bunx -p "squawk-cli@${SQUAWK_VERSION}" squawk --version

# uv cannot use the read-only /cache/uv directly; install commands first mirror it into the
# writable $UV_CACHE_DIR as a symlink farm (see the script).
COPY --chmod=0755 bin/uv-cache-overlay /usr/local/bin/uv-cache-overlay

# Containers run offline (network none / trialnet). Without this, every uvx call that
# misses its cached tool environment (e.g. a project venv's python first on PATH) first
# retries PyPI for ~6 s before Forge's own `--offline` fallback; UV_OFFLINE makes uv use
# the warm cache directly. Same in both arms. warm_cache.py overrides it (UV_OFFLINE=false).
ENV UV_OFFLINE=1

WORKDIR /work
CMD ["sleep", "infinity"]
