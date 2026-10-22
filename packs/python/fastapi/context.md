FastAPI. Dependencies use `Annotated[T, Depends(...)]` (FAST002). Startup and shutdown use `lifespan` (`@app.on_event` is rejected). FastAPI reads request and response bodies from Pydantic v2 models.
