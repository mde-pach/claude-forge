These are this project's defaults. When the task or the user explicitly asks for something different, do what they ask and point out the conflict; never override an explicit instruction with these defaults.

Alembic with an async engine: `alembic/env.py` runs migrations through `run_async_migrations()` and `connection.run_sync()`. New revisions: `alembic revision --autogenerate -m "<change>"`. Committed revisions are never edited.
Alembic owns the schema: tables are created by revisions, and `metadata.create_all()` outside tests is rejected.
