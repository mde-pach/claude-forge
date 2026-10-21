# syntax=docker/dockerfile:1
FROM python:3.13-slim AS build
WORKDIR /app
COPY --from=ghcr.io/astral-sh/uv:0.11.3 /uv /bin/uv
COPY pyproject.toml uv.lock ./
RUN --mount=type=cache,target=/root/.cache/uv uv sync --locked --no-dev --no-install-project
COPY app ./app

FROM python:3.13-slim
WORKDIR /app
RUN useradd --system --uid 10001 app
COPY --from=build /app /app
USER 10001
CMD ["/app/.venv/bin/python", "-m", "app"]
