## 2. Stack and platform

| Slot | Python | TypeScript |
|---|---|---|
| Package manager | uv | Bun |
| Framework | FastAPI | Next.js 16 (App Router) + React 19; Astro (+ React islands) |
| Data | SQLAlchemy 2.0 async, typed declarative + Alembic async | Prisma 7 |
| Validation | Pydantic v2 | Zod |
| Format + lint | ruff | Biome 2 |
| Typecheck | ty (Forge's pinned ty, §15.2) | TypeScript 7 `tsc` |
| Unit tests | pytest | Vitest |
| E2E tests | — | Playwright |
| Database | PostgreSQL | PostgreSQL |

- One application per repository.
- Platforms: macOS, Linux, WSL2.
- Claude Code ≥ 2.1.284. `forge apply` reads `claude --version` and refuses below it.
- A project that uses other tools is never broken: rules whose tool is absent are reported as "not enforced here" (§11).
