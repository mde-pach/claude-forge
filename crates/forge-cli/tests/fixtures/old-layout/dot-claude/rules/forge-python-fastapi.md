These are this project's defaults. When the task or the user explicitly asks for something different, do what they ask and point out the conflict; never override an explicit instruction with these defaults.

FastAPI. Dependencies use `Annotated[T, Depends(...)]` (FAST002). Startup and shutdown use `lifespan` (`@app.on_event`, `on_startup=` and `on_shutdown=` are rejected). FastAPI reads request and response bodies from Pydantic v2 models.
`FastAPI(debug=True)` is rejected outside tests. Tests enter `with TestClient(app) as client:` so the lifespan runs; async tests use `httpx.AsyncClient(transport=httpx.ASGITransport(app=app), base_url="http://test")` (TestClient in an `async def` is rejected).
HTTP Basic credentials (`HTTPBasicCredentials.username` / `.password`) are compared with `secrets.compare_digest`, never `==`.
