Python project managed with uv. Tools run from `.venv/bin`. Type checking: basedpyright.
Inside `async def`, the checks reject blocking calls that ruff's ASYNC210, ASYNC230 and ASYNC251 detect (sync HTTP clients, `open()`, `time.sleep`).
Ignore comments name their rule (`# pyright: ignore[ruleName]`), and a changed file may not gain suppression comments.
A web endpoint that sends an HTTP request to a URL taken from its parameters checks the host against an allow-list first (`is_allowed_url()`, `ALLOWED_HOSTS`); a miss gets advice, not a block.
