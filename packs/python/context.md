Parameters and return types are annotated.
Inside `async def`, no blocking calls: sync HTTP clients, `time.sleep`, `open()` and `pathlib` I/O, blocking `subprocess`, `input()`. A caught `asyncio.CancelledError` is re-raised, and a `try` around a task group catches with `except*`.
Ignore comments name their rule (`# ty: ignore[rule-name]`), and a changed file may not gain suppression comments.
Before writing a utility, search the codebase for an existing helper (warn-once and logging helpers, `utils` functions) and reuse it instead of adding a near-duplicate.
