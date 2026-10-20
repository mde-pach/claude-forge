# Clean container for §14.4: Claude Code, git, Bun, uv, PostgreSQL; no Forge.
# The sample repositories are prepared with forge apply on the host (prepare.sh)
# and copied in already committed.
FROM ubuntu:24.04
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates curl git unzip postgresql postgresql-contrib nodejs npm python3 \
    && rm -rf /var/lib/apt/lists/*
RUN npm install -g @anthropic-ai/claude-code@latest \
    && curl -fsSL https://bun.sh/install | bash \
    && curl -LsSf https://astral.sh/uv/install.sh | sh
ENV PATH=/root/.bun/bin:/root/.local/bin:$PATH
COPY work/ /work/
COPY run.ts setup-repos.sh /e2e/
WORKDIR /work
CMD ["sh", "-c", "service postgresql start && sh /e2e/setup-repos.sh && bun /e2e/run.ts"]
