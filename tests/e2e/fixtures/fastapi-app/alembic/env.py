import asyncio

from alembic import context
from sqlalchemy.ext.asyncio import async_engine_from_config

from app.models import Base

target_metadata = Base.metadata


def do_run_migrations(connection) -> None:
    context.configure(connection=connection, target_metadata=target_metadata)
    with context.begin_transaction():
        context.run_migrations()


async def run_async_migrations() -> None:
    engine = async_engine_from_config(context.config.get_section(context.config.config_ini_section, {}), prefix="sqlalchemy.")
    async with engine.connect() as connection:
        await connection.run_sync(do_run_migrations)
    await engine.dispose()


asyncio.run(run_async_migrations())
