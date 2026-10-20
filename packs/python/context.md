Python project managed with uv. Tools run from `.venv/bin`. Type checking: basedpyright.
Async code never blocks the event loop: no `requests`, `time.sleep` or blocking `open()` inside `async def`.
Ignore comments name their rule (`# pyright: ignore[ruleName]`).
