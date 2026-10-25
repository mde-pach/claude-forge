Python project managed with uv. Tools run from `.venv/bin`. Type checking: basedpyright.
Inside `async def`, the checks reject blocking calls that ruff's ASYNC210, ASYNC230 and ASYNC251 detect (sync HTTP clients, `open()`, `time.sleep`).
Ignore comments name their rule (`# pyright: ignore[ruleName]`), and a changed file may not gain suppression comments.
Before writing a utility, search the codebase for an existing helper (warn-once and logging helpers, `utils` functions) and reuse it instead of adding a near-duplicate.
