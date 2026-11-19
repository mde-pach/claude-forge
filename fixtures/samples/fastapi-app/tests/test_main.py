from app.main import health


async def test_health() -> None:
    assert await health() == {"status": "ok"}
