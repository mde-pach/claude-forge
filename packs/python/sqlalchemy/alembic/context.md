Alembic with an async engine: `alembic/env.py` runs migrations through `run_async_migrations()` and `connection.run_sync()`. New revisions: `alembic revision --autogenerate -m "<change>"`. Committed revisions are never edited.
Alembic owns the schema: tables are created by revisions, and `metadata.create_all()` outside tests is rejected.
