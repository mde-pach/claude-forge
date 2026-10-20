#!/bin/sh
# Container side: installs each sample's own dependencies and creates the local
# databases. Forge is not installed in the container.
set -eu
su postgres -c "psql -c \"CREATE USER app WITH PASSWORD 'app' SUPERUSER\"" || true
for db in app app_shadow; do su postgres -c "createdb -O app $db" || true; done
cd /work/fastapi-app
uv lock && uv sync && uv add --dev ruff basedpyright pytest pytest-asyncio
uv run alembic upgrade head
git add -A && git -c user.name=e2e -c user.email=e2e@example.com commit -qm "deps" || true
cd /work/nextjs-prisma-app
bun install && bunx prisma generate
git add -A && git -c user.name=e2e -c user.email=e2e@example.com commit -qm "deps" || true
