Typer. Parameters are declared with `Annotated`: `name: Annotated[str, typer.Option(help=...)] = "x"`, never `name: str = typer.Option(...)`; the default is the parameter's default value.
