### python

| Rule | Targets |
|---|---|
| `python/format` | `check` format: `run: ruff format --check .`, `changed_run: ruff format {files}`, `format_stdin: ruff format --stdin-filename {file} -` |
| `python/lint` | `check` lint: `run: ruff check .`, `changed_run: ruff check --output-format concise {files}`, `fix_run: ruff check --fix --fixable I,UP --exit-zero --show-fixes {files}` (sorted imports and pyupgrade, when the project selects them; never F401 / F841), `config_files: ["**/pyproject.toml"]` (`RUF200`) |
| `python/valid-pyproject` | `tool` ruff `RUF200` (`[project]` fields typed as PEP 621 says; a changed `pyproject.toml` is linted by `python/lint`'s `config_files`) |
| `python/typecheck` | `check` typecheck: `run: ty check --output-format concise` (Forge's pinned ty, §15.2, on the project's Python environment, §9.1); errors the base already has are not the change's (§10.1 step 4) |
| `python/no-explicit-any` | `tool` ruff `ANN401` (ty has no equivalent) |
| `python/no-bare-except` | `tool` ruff `E722` |
| `python/no-unscoped-ignore` | `tool` ty `blanket-ignore-comment`; `tool` ruff `PGH003` |
| `python/no-unknown` | `tool` ruff `ANN001`, `ANN002`, `ANN003`, `ANN201`, `ANN202`, `ANN204`, `ANN205`, `ANN206`: parameters and return types annotated, so no receiver is Unknown (§15.1); an unannotated local's type is inferred (ty has no equivalent of `reportUnknownVariableType`) |
| `python/no-useless-cast` | `tool` ty `redundant-cast`, `disjoint-cast` |
| `python/no-deprecated` | `tool` ty `deprecated` (PEP 702 and typeshed deprecations) |
| `python/no-blocking-in-async` | `tool` ruff `ASYNC210`, `ASYNC230`, `ASYNC251`, `ASYNC212`, `ASYNC220`, `ASYNC221`, `ASYNC222`, `ASYNC240`, `ASYNC250`, `ASYNC100`, `ASYNC105`, `ASYNC110` (not `ASYNC109`: it keys on a parameter named `timeout`) |
| `python/no-tls-bypass` | `tool` ruff `S501`, `S323`, `S502` |
| `python/no-shell-true` | `tool` ruff `S602`, `S605` |
| `python/no-stale-ignore` | `tool` ty `unused-ignore-comment`, `unused-type-ignore-comment` |
| `python/no-unawaited-coroutine` | `tool` ty `unused-awaitable` |
| `python/no-forgotten-call` | `tool` ty `redundant-condition`, `truthiness-test-of-callable`, `truthiness-test-of-iterable` |
| `python/no-possibly-missing-submodule` | `tool` ty `possibly-missing-submodule` |
| `python/override-decorator` | `tool` ty `missing-override-decorator` |
| `python/generic-type-args` | `tool` ty `missing-type-argument` |
| `python/typed-decorators` | `tool` ty `dynamic-function-decorator-return` |
| `python/no-get-event-loop` | `tool` ruff `TID251`, banned-api `asyncio.get_event_loop` |
| `python/no-ensure-future` | `tool` ruff `TID251`, banned-api `asyncio.ensure_future` |
| `python/no-nest-asyncio` | `tool` ruff `TID251`, banned-api `nest_asyncio` |
| `python/no-async-timeout` | `tool` ruff `TID251`, banned-api `async_timeout` |
| `python/no-pytz` | `tool` ruff `TID251`, banned-api `pytz` |
| `python/no-python-jose` | `tool` ruff `TID251`, banned-api `jose` |
| `python/no-passlib` | `tool` ruff `TID251`, banned-api `passlib` |
| `python/no-pkg-resources` | `tool` ruff `TID251`, banned-api `pkg_resources` |
| `python/no-removed-stdlib` | `tool` ruff `TID251`, banned-api: the modules removed in Python 3.12 (`distutils`, `imp`, `asynchat`, `asyncore`, `smtpd`) and 3.13 (PEP 594's 19, `lib2to3`) |
| `python/aware-datetime` | `tool` ruff `DTZ005`, `DTZ001`, `DTZ002`, `DTZ006`, `DTZ011`, `DTZ012`, `DTZ901` (not `DTZ007`: it flags date-only `strptime(…).date()`) |
| `python/no-new-suppressions` | `grep` no-increase: `# noqa`, `# ty: ignore`, `# type: ignore`, `# pyright: ignore`, `# pragma: no cover`, `# pragma: no branch` (suppression budget) |
| `python/config-files` | `permission` ask: `Edit(**/ruff.toml)`, `Edit(**/.ruff.toml)`, `Edit(**/ty.toml)`, `Edit(**/mypy.ini)`, `Edit(**/.mypy.ini)`, `Edit(**/pyrightconfig.json)` |
| `python/config-ratchet` | `check` lint: `forge_ratchet python ratchet.py` (§10.4), `when_changed`: the Python config files |
| `python/no-silent-except` | `tool` ruff `S110`, `S112` |
| `python/no-blind-except` | `tool` ruff `BLE001` |
| `python/no-blanket-noqa` | `tool` ruff `PGH004` |
| `python/no-unused-noqa` | `tool` ruff `RUF100` |
| `python/no-commented-out-code` | `tool` ruff `ERA001` |
| `python/no-print` | `tool` ruff `T201` (snippet: per-file-ignores for `scripts/**` and CLIs) |
| `python/no-dangling-task` | `tool` ruff `RUF006` |
| `python/log-exception` | `ast-grep` forbid: `$L.error(…)` without `exc_info=` inside an `except` handler (not in a function, lambda or class defined there), where `L` is the standard library's `logging.Logger` or `LoggerAdapter` (§15.1, no `examples.deps`); replaces ruff `TRY400`, which picks loggers by their names |
| `python/lazy-log-args` | `ast-grep` forbid: `$L.debug/info/warning/error/exception/critical(…)` whose first argument (`$L.log(…)`'s second) is an f-string, a `.format()` call or a `%`-formatted string, where `L` is `logging.Logger` / `LoggerAdapter` (§15.1); not ruff `G001`–`G004`, which pick loggers by their names |
| `python/archive-extract` | `ast-grep` forbid: `$T.extractall(…)` / `$T.extract(…)` where `T` is `tarfile.TarFile` (§15.1), and `…unpack_archive(…)` declared by `shutil` (§15.6), without `filter=`; not ruff `S202`, which flags any `.extractall` and `zipfile` |
| `python/hmac-compare` | `ast-grep` forbid: a `==` / `!=` comparison with `$H.hexdigest()` / `$H.digest()` where `H` is `hmac.HMAC` (§15.1), or a call declared as `hmac.digest` (§15.6) |
| `python/no-ssl-verify-off` | `ast-grep` forbid: `$C.check_hostname = False`, or `$C.verify_mode = …CERT_NONE` declared by `ssl` (§15.6), where `C` is `ssl.SSLContext` (§15.1) |
| `python/no-hardcoded-password` | `tool` ruff `S105`, `S106` (snippet: off under `tests/**`) |
| `python/no-sql-format` | `tool` ruff `S608` |
| `python/http-timeout` | `tool` ruff `S113` (requests without `timeout`; httpx with `timeout=None`) |
| `python/llm-client-timeout` | `ast-grep` forbid: a call typed `anthropic` or `openai` (§15.1: a client constructor, `with_options()`, a request) with `timeout=None` |
| `python/no-weak-hash` | `tool` ruff `S324` |
| `python/no-pickle` | `tool` ruff `S301`, `S302` |
| `python/no-unsafe-yaml` | `tool` ruff `S506` |
| `python/jinja-autoescape` | `tool` ruff `S701` |
| `python/no-stub` | `ast-grep` forbid, not in `*.pyi` or migrations: a function whose body is only `pass` / `...` (docstrings and comments aside), or `raise NotImplementedError`, outside `@abstractmethod`, `@overload` and `Protocol` / `ABC` classes |
| `python/stub-sync` | `check` lint, advisory (§10.7): `forge_stub_sync` on `*.py` |
| `python/no-placeholder-comment` | `grep` forbid (case-insensitive): comments such as `TODO: implement`, "in a real implementation", "placeholder logic", "stub for now" |
| `python/env-example` | `check` lint: `forge_env_example py`, not with `--all`: a field added to a pydantic-settings `BaseSettings` class (`env_prefix`, `alias`, `case_sensitive` applied) appears in the nearest directory's `.env.example`, `.env.template`, `.env.sample`, `.env.dist` or `.env.<name>.example` (a `# KEY=` line counts): always without a default, also with one when the template lists at least half of the class's variables; a template listing none of them is not about that class |
| `python/taskgroup-except-star` | `ast-grep` forbid: a plain `except SomeError` (not `except*`, `Exception`, `BaseException`, `ExceptionGroup`, `BaseExceptionGroup`) on a `try` whose body is only `async with <cm> as …:`, where `<cm>` is typed `asyncio` / `anyio` `TaskGroup` (§15.1) or is literally `trio.open_nursery(…)` (typed `AbstractAsyncContextManager[Nursery]`) |
| `python/no-swallowed-cancel` | `ast-grep` forbid: an `except` catching `asyncio.CancelledError` (or `CancelledError` imported from `asyncio`), alone or in a tuple, with no `raise` in the handler (nested functions aside) |
| `python/subprocess-check` | `tool` ruff `PLW1510` |
| `python/no-mutable-defaults` | `tool` ruff `B006`, `B039`, `RUF008`, `RUF009`, `RUF033` |
| `python/type-checking-imports` | `tool` ruff `TC004` |
| `python/raise-from` | `tool` ruff `B904` |
| `python/no-raise-vanilla` | `tool` ruff `TRY002` |
| `python/no-jump-in-finally` | `tool` ruff `B012` |
| `python/no-root-logger` | `tool` ruff `LOG015`, `LOG001`, `LOG002` (not `LOG004`, `LOG007`, `LOG014`, `PLE1205`, `PLE1206`: they pick loggers by receiver name) |
| `python/open-with-context` | `tool` ruff `SIM115` |
| `python/decimal-from-str` | `tool` ruff `RUF032` |
| `python/loop-closure` | `tool` ruff `B023` |
| `python/no-eval` | `tool` ruff `S307`, `S102` |
| `python/no-debugger` | `tool` ruff `T100` |
| `python/no-unsafe-markup` | `tool` ruff `S704` |
| `python/ruff-config-current` | `check` lint: `ruff check --exit-zero` prints no `warning: … deprecat…` line for the project's config (deprecated top-level linter settings, deprecated options); `when_changed`: `pyproject.toml`, `ruff.toml`, `.ruff.toml` |
| `python/project-table` | `check` lint: the root `pyproject.toml` has `[project]` `requires-python` (`forge_toml_get`); `when_changed: [pyproject.toml]` |
| `python/no-preexec-fn` | `tool` ruff `PLW1509` |
| `python/no-useless-lock` | `tool` ruff `PLW2101` |
| `python/no-cached-method` | `tool` ruff `B019` |
| `python/self-type` | `tool` ruff `PYI019`, `PYI034` |
| `python/annotations-api` | `tool` ruff `RUF063` |
| `python/typeis-over-typeguard` | `ast-grep` advise: a function whose return annotation is `TypeGuard[…]` (`typing.` / `typing_extensions.` or bare) |
| `python/no-useless-try` | `tool` ruff `TRY203`, `TRY201` |
| `python/misplaced-bare-raise` | `tool` ruff `PLE0704` |
| `python/contextmanager-finally` | `ast-grep` forbid: in a function decorated `@contextmanager` / `@asynccontextmanager` (bare or `contextlib.`), a `yield` statement directly in its body followed by another statement |
| `python/with-suffix-dot` | `tool` ruff `PTH210` |
| `python/no-commonprefix` | `tool` ruff `RUF071` |
| `python/strict-zip` | `tool` ruff `B905` |
| `python/weak-crypto` | `tool` ruff `S304`, `S305`, `S505` (the `cryptography` package) |
| `python/no-bidi-chars` | `tool` ruff `PLE2502`, `PLE2515` |
| `python/no-unused-args` | `tool` ruff `ARG001`, `ARG002` |
| `python/no-global-statement` | `tool` ruff `PLW0603` |
| `python/env-misuse` | `tool` ruff `B003`, `PLW1508`, `PLW1507` |
| `python/no-setup-py` | `permission` ask: `Write(**/setup.py)`, `Write(**/setup.cfg)` |
| `python/no-ruff-overlap` | `grep` forbid (case-insensitive) in `pyproject.toml` and `.pre-commit-config.yaml`: `black`, `isort`, `flake8`, `autopep8`, `pyupgrade`, `autoflake` as a quoted dependency string, or their pre-commit hook repository URLs |
| `python/no-dev-server-in-image` | `grep` forbid on `@dockerfile` and `@procfile` (§15.4): a `CMD` / `ENTRYPOINT` / Procfile process line running `--reload`, `fastapi dev` or `flask … --debug` |

Every banned-api rule shares ruff's `TID251`: each rule's examples import only its own module, so the audit tells them apart, and `forge status` merges their tables (§11).

context.md: uv and `.venv/bin`, ty, annotated parameters and returns; no blocking calls in `async def`, `CancelledError` re-raised, `except*` around task groups; ignore comments name their rule; search for an existing helper (warn-once, logging, utils) before writing one.

### python/uv

| Rule | Targets |
|---|---|
| `python/uv/lock-sync` | `check` lint: `run: uv lock --check`, `when_changed: [pyproject.toml]` |
| `python/uv/no-pip` | `guard` ask: `pip install`, `pip3 install`, `uv pip install`, `python -m pip install`; `poetry add` / `install`, `pipenv install`, `pdm add` / `install`, `conda` / `mamba` / `micromamba install`, `python -m venv` / `virtualenv`, `virtualenv` |
| `python/uv/dependency-groups` | `grep` forbid in `pyproject.toml`: a `dev-dependencies =` key (`[tool.uv]`; development dependencies go in `[dependency-groups]`, PEP 735) |
| `python/uv/no-requirements-txt` | `permission` ask: `Write(**/requirements*.txt)`, `Edit(**/requirements*.txt)` (`uv export` through Bash is not asked) |
| `python/uv/exclude-newer` | `check` lint: `forge_uv_cooldown`: `[tool.uv]` (or `uv.toml`) `exclude-newer` is a relative duration of at least 3 days (`"7 days"`, `P1W`) and `required-version` is `>=0.9.17`; `when_changed`: `pyproject.toml`, `uv.toml`, `uv.lock` |

### python/fastapi

| Rule | Targets |
|---|---|
| `python/fastapi/no-on-event` | `ast-grep` forbid: a call typed `fastapi` `FastAPI` / `APIRouter` (§15.1) with `on_startup=` or `on_shutdown=`; `@app.on_event` is deprecated and reported by `python/no-deprecated` |
| `python/fastapi/annotated-depends` | `tool` ruff `FAST002` |
| `python/fastapi/cors-credentials-wildcard` | `ast-grep` forbid: `add_middleware(CORSMiddleware, …)` with both `allow_origins=["*"]` and `allow_credentials=True` |
| `python/fastapi/unused-path-param` | `tool` ruff `FAST003` |
| `python/fastapi/no-debug-true` | `ast-grep` forbid, not in tests: a call typed `starlette` `Starlette` (`FastAPI` included) with `debug=True` |
| `python/fastapi/no-raw-request-body` | `ast-grep` advise: `await $REQ.json()`, `await $REQ.form(…)`, `await $REQ.body()`, `$REQ.stream()` on a `starlette` `Request` |
| `python/fastapi/testclient-lifespan` | `ast-grep` advise, in `@python-tests`: a call typed `starlette` `TestClient` outside a `with` item |
| `python/fastapi/no-testclient-in-async-test` | `ast-grep` forbid, in `@python-tests`: a call typed `starlette` `TestClient` inside an `async def` |
| `python/fastapi/deprecated-params` | `ast-grep` forbid: a call with `regex=` or `example=` whose callee is declared (§15.6) by `fastapi` as `Query`, `Path`, `Header`, `Cookie`, `Body`, `Form` or `File` (they return `Any`: the call's type says nothing); a call with `root_path=` whose type is `fastapi`'s `FastAPI` |

context.md: "FastAPI. Dependencies use `Annotated[T, Depends(...)]`. Startup and shutdown use `lifespan`. Request and response bodies are Pydantic v2 models." No debug mode outside tests; `with TestClient(app)` so the lifespan runs, `httpx.AsyncClient` with `ASGITransport` in async tests.

### python/litestar

Detected by `litestar` (pypi).

| Rule | Targets |
|---|---|
| `python/litestar/sync-to-thread` | `ast-grep` forbid: a sync `def` decorated with a call typed `litestar` `HTTPRouteHandler` (§15.1: `@get`, `@post`, …, `@route`) without `sync_to_thread=` |
| `python/litestar/no-debug-true` | `ast-grep` forbid, not in tests: a call typed `litestar` `Litestar` with `debug=True` |

context.md: `sync_to_thread` stated on sync handlers; no debug mode outside tests.

### python/flask

Detected by `flask` (pypi).

| Rule | Targets |
|---|---|
| `python/flask/no-debug-run` | `tool` ruff `S201` |
| `python/flask/no-template-string-injection` | `ast-grep` forbid: a call whose callee is declared (§15.6) by `flask` as `render_template_string` and whose first argument is an f-string with an interpolation, a binary operation (`+`, `%`) or a `.format()` call |

### python/flask/sqlalchemy

Detected by `flask-sqlalchemy` (pypi).

| Rule | Targets |
|---|---|
| `python/flask/sqlalchemy/no-model-query` | `ast-grep` forbid: `$M.query` where `$M` is typed `flask_sqlalchemy` (§15.1: a `db.Model` class); `db.session.query()` is `python/sqlalchemy/no-query-api` |

context.md: SQLAlchemy 2.0 queries through `db.session`, `db.get_or_404()`, `db.paginate()`.

| `python/fastapi/basic-auth-compare-digest` | `ast-grep` forbid: `==` / `!=` with `$C.username` or `$C.password` as an operand, where `C` is typed `fastapi` `HTTPBasicCredentials` (§15.1) |

context.md: "FastAPI. Dependencies use `Annotated[T, Depends(...)]`. Startup and shutdown use `lifespan`. Request and response bodies are Pydantic v2 models." HTTP Basic credentials are compared with `secrets.compare_digest`.

### python/sqlalchemy

| Rule | Targets |
|---|---|
| `python/sqlalchemy/no-query-api` | `ast-grep` forbid: `$S.query($$$)` |
| `python/sqlalchemy/no-declarative-base` | `ast-grep` forbid: `declarative_base($$$)` |
| `python/sqlalchemy/no-backref` | `ast-grep` forbid: `relationship($$$, backref=$_, $$$)` |
| `python/sqlalchemy/mapped-column` | `ast-grep` forbid: `$N = Column($$$)` inside a class body |
| `python/sqlalchemy/no-raw-execute` | `ast-grep` forbid: `$X.execute("$$$")` with a string literal |
| `python/sqlalchemy/async-expire-on-commit` | `ast-grep` forbid: `async_sessionmaker(…)` without `expire_on_commit=False` |
| `python/sqlalchemy/no-text-fstring` | `ast-grep` forbid: `text()` whose SQL is an f-string, `.format()`, `%` or `+` concatenation |
| `python/sqlalchemy/no-concurrent-session` | `ast-grep` forbid: `asyncio.gather(…)` with two calls on one receiver typed `sqlalchemy` `AsyncSession` / `async_scoped_session` (§15.1) |
| `python/sqlalchemy/no-lazy-dynamic` | `ast-grep` forbid: a call typed in `sqlalchemy.orm` (`relationship`, `backref`, `dynamic_loader`) with `lazy="dynamic"` |
| `python/sqlalchemy/session-context` | `ast-grep` advise: a call typed `sqlalchemy` `Session` / `AsyncSession` (`Session(engine)`, a sessionmaker's call) outside a `with` item, a `return` or a `yield` |
| `python/sqlalchemy/asyncio-extra` | `check` lint: when a tracked or new `*.py` imports `sqlalchemy.ext.asyncio`, a `pyproject.toml` or `requirements*.txt` names `sqlalchemy[asyncio]` or `greenlet`; `when_changed`: those files |
| `python/sqlalchemy/fk-index` | `check` lint, advisory (§10.7): `forge_advise forge_helper python fk_index.py` (§13.4), skipped for a MySQL / MariaDB `forge_db_url` |

context.md: SQLAlchemy 2.0 async models, queries and sessions (opened with `async with`, `sqlalchemy[asyncio]` declared); no lazy loading in async sessions (`selectinload()`, `lazy="raise"`, `WriteOnlyMapped` instead of `lazy="dynamic"`), one statement at a time per session; values reach `text()` as bound parameters, with f-strings, `.format()`, `%`, `+` in `text()` and literal strings in `.execute()` rejected, and statements that cannot take parameters (`SET`, `SET LOCAL`) free to embed a validated value.

### python/sqlalchemy/alembic

| Rule | Targets |
|---|---|
| `python/sqlalchemy/alembic/check` | `check` migration: `run: alembic check`, `when_changed: [**/models/**, **/models.py, alembic/versions/**]`, `requires_local_db: true` |
| `python/sqlalchemy/alembic/single-head` | `check` migration: `run: test "$(alembic heads | wc -l)" -le 1`, `when_changed: [alembic/versions/**]` |
| `python/sqlalchemy/alembic/no-downgrade-base` | `guard` deny: `alembic downgrade base` |
| `python/sqlalchemy/alembic/downgrade` | `guard` ask: `alembic downgrade`, `alembic stamp` |
| `python/sqlalchemy/alembic/migration-lint` | `check` migration: each changed revision's upgrade rendered offline (`alembic upgrade <down_revision>:<revision> --sql`) and linted with Squawk (§13.1); `--all`: the head revision; merge revisions are skipped; `when_changed: [alembic/versions/**]` |
| `python/sqlalchemy/alembic/model-registered` | `check` lint on `**/models/**`: `forge_models_registered`: a module of a models package with a mapped class (`__tablename__`) is named in the package's `__init__.py` when that file imports other table modules of the package (the registry), or in `alembic/env.py`; otherwise `Base.metadata` lacks the table and `alembic check` passes without its migration |
| `python/sqlalchemy/alembic/fk-index` | `check` lint on `alembic/versions/**`, advisory (§10.7): `forge_advise forge_alembic_fk_index`: in `upgrade()`, each column given a foreign key (`sa.Column(…, sa.ForeignKey(…))`, `sa.ForeignKeyConstraint([…])`, `op.create_foreign_key(…, […], …)`) is the first column of an `op.create_index`, `sa.Index`, primary key or unique constraint or `CREATE INDEX`, or has `index=True`, `unique=True` or `primary_key=True`; skipped for a MySQL / MariaDB database URL |
| `python/sqlalchemy/alembic/no-create-all` | `ast-grep` forbid, not in tests: `$M.create_all` with `$M` typed `sqlalchemy` `MetaData` (§15.1: `Base.metadata`, `SQLModel.metadata`; the call and `run_sync(…create_all)`) |
| `python/sqlalchemy/alembic/sqlite-batch` | `check` lint on `alembic/versions/**`: `forge_alembic_batch`: when `alembic/env.py` sets `render_as_batch=True` or `alembic.ini` has a sqlite URL, no `op.alter_column`, `op.drop_column`, `op.create_foreign_key`, `op.drop_constraint`, `op.create_unique_constraint`, `op.create_check_constraint`, `op.create_primary_key` outside `op.batch_alter_table()`, except under an `if` on the dialect |
| `python/sqlalchemy/alembic/data-migration-downgrade` | `check` lint on `alembic/versions/**`, advisory (§10.7): `forge_advise forge_alembic_data_downgrade`: an `upgrade()` that changes rows (`UPDATE … SET`, `INSERT INTO`, `DELETE FROM` with `execute(`, `sa.update/insert/delete(…)`, `.update()/.insert()/.delete()`, `op.bulk_insert(`, `Session(`) has a `downgrade()` that changes rows the same ways or has a `raise` statement; reported at the `downgrade` line |
| `python/sqlalchemy/alembic/applied-migrations` | `check` migration: `forge_applied_migrations @alembic-migrations` (`alembic.ini` `script_location`, §15.4): a revision that exists in the base is not modified or deleted (not with `--all`) |

The migration `check` lint rules above examine the changed revisions, and with `--all` only the head revisions. A required column added without a default (`op.add_column(…, nullable=False)` without `server_default`) is already caught by `migration-lint` (Squawk `adding-required-field`, §13.1).

context.md: "Alembic with an async engine: `alembic/env.py` runs migrations through `run_async_migrations()` and `connection.run_sync()`. New revisions: `alembic revision --autogenerate -m \"<change>\"`. Committed revisions are never edited."; no `metadata.create_all()` outside tests.

### python/pydantic

| Rule | Targets |
|---|---|
| `python/pydantic/v2-api` | `ast-grep` forbid: `class Config:` in a class whose superclass is typed `pydantic` `BaseModel` (§15.1; `BaseSettings` derives from it), `json_encoders=` in a call declared `pydantic` `ConfigDict` or `pydantic_settings` `SettingsConfigDict` (§15.6) or as a key of a `model_config = {…}` dict, and v1-only arguments (`env`, `regex`, `const`, `min_items`, `max_items`, `unique_items`, `allow_mutation`, `example`) of a call declared `pydantic` `Field`; `.dict()`, `.json()`, `parse_obj`, `@validator` are ty's `deprecated` (`python/no-deprecated`) |
| `python/pydantic/no-v1-namespace` | `tool` ruff `TID251`, banned-api `pydantic.v1` |
| `python/pydantic/typeadapter-module-level` | `ast-grep` advise: a call typed `pydantic` `TypeAdapter` inside a function not decorated with `cache`, `lru_cache` or `cached_property` |

context.md: "Pydantic v2: `model_validate`, `model_dump`, `model_config = ConfigDict(...)`, `@field_validator`, `@model_validator`, `Field(pattern=...)`, `TypeAdapter(T).validate_python(...)` (not `parse_obj_as`)."

### python/pydantic/settings

Detected by `pydantic-settings` (pypi).

| Rule | Targets |
|---|---|
| `python/pydantic/settings/no-load-dotenv` | `tool` ruff `TID251`, banned-api `dotenv.load_dotenv` |

### python/attrs

Detected by `attrs` (pypi).

| Rule | Targets |
|---|---|
| `python/attrs/next-gen-api` | `tool` ruff `TID251`, banned-api `attr.s`, `attr.attrs`, `attr.dataclass`, `attr.ib`, `attr.attrib` |

### python/pytest

| Rule | Targets |
|---|---|
| `python/pytest/run` | `check` test: `run: pytest -q`, `changed_run`: changed test files plus `tests/**/test_<module>.py` for each changed module and, at Stop, the test files edited during the turn (`<snapshot>/touched`, §10.5) even when the edit was reverted; `pytest --testmon` when the project depends on pytest-testmon; timeout 300 s |
| `python/pytest/no-new-skip` | `ast-grep` no-increase on test files: `@pytest.mark.skip($$$)`, `@pytest.mark.skipif($$$)`, `@pytest.mark.xfail($$$)`, `pytest.skip($$$)`, `@unittest.skip($$$)`, `pytest.importorskip($$$)` |
| `python/pytest/no-hook-tamper` | `ast-grep` advise on test files and `conftest.py`: a `def` of a result-rewriting hook (`pytest_runtest_makereport`, `pytest_report_teststatus`, `pytest_runtest_call`, `pytest_pyfunc_call`, `pytest_runtest_protocol`, `pytest_terminal_summary`, `pytest_collection_modifyitems`) |
| `python/pytest/no-exitstatus` | `ast-grep` advise, in `@python-tests`: `$S.exitstatus = $V` where `S` is pytest's `Session` (`_pytest.main`, §15.1) |
| `python/pytest/no-exit-zero` | `ast-grep` forbid on test files and `conftest.py`: `sys.exit(0)`, `os._exit(0)`, `pytest.exit(…, returncode=0)` |
| `python/pytest/no-fewer-asserts` | `ast-grep` no-decrease on test files: `assert $$$` |
| `python/pytest/no-test-env-branch` | `ast-grep` forbid on non-test files: `PYTEST_CURRENT_TEST` lookups, `"pytest" in sys.modules` |
| `python/pytest/no-blind-raises` | `tool` ruff `B017` |
| `python/pytest/no-tautological-assert` | `ast-grep` forbid on test files: `assert` on a literal (`True`, a number, a string), `assert $X == $X`, `assert $X is $X` |
| `python/pytest/no-mock-increase` | `ast-grep` no-increase on test files: calls to `patch`, `patch.object`, `patch.dict`, `patch.multiple`, `Mock`, `MagicMock`, `AsyncMock`, `NonCallableMock`, `create_autospec` (bare, `mock.`, `unittest.mock.`, or through an alias: `from unittest.mock import patch as p`, `import unittest.mock as um`, `from unittest import mock as m`), and `mocker.patch*`, `mocker.spy`, `mocker.stub`, `mocker.Mock*`; `monkeypatch` is not counted (pytest's way to set configuration) |
| `python/pytest/no-test-edits` | `check` lint: `forge_test_edits` on `**/test_*.py`, `**/*_test.py`, `**/conftest.py`; edits ask (§10.5); `review`: is the change required by the user's request, or does it weaken the tests |
| `python/pytest/untested-change` | `check` lint, advisory (§10.7): `forge_untested_change` on non-test `*.py` (not migrations) |
| `python/pytest/django-deprecations-error` | `check` lint: `forge_helper python pytest_filterwarnings.py error::django.utils.deprecation.RemovedInNextVersionWarning django` (§13.4), `when_changed`: the pytest config files; only with `django` (`when`) |
| `python/pytest/drf-deprecations-error` | `check` lint: `forge_helper python pytest_filterwarnings.py error::rest_framework.deprecation.RemovedInNextDRFVersionWarning djangorestframework` (§13.4), `when_changed`: the pytest config files; only with `djangorestframework` (`when`) |
| `python/pytest/pydantic-deprecations-error` | `check` lint: `forge_helper python pytest_filterwarnings.py error::pydantic.warnings.PydanticDeprecationWarning pydantic` (§13.4), `when_changed`: the pytest config files; only with `pydantic >= 2.0.0` (`when`) |

context.md: tests assert behaviour, never weakened; "When the task changes behaviour an existing test asserts, update that test in place with Edit or Write (keep its name; Forge asks for approval) and say so in your summary. Otherwise never edit an existing test to make the code pass."

### python/pytest/asyncio

Detected by `pytest-asyncio` (pypi). Rules on test files (`**/test_*.py`, `**/*_test.py`, `**/conftest.py`).

| Rule | Targets |
|---|---|
| `python/pytest/asyncio/no-event-loop-fixture` | `ast-grep` forbid: a function `event_loop` decorated `@pytest.fixture` / `@pytest_asyncio.fixture`; an `event_loop` parameter of a function whose name starts with `test` |
| `python/pytest/asyncio/no-loop-policy-fixture` | `ast-grep` forbid: a function `event_loop_policy` decorated `@pytest.fixture` / `@pytest_asyncio.fixture` |
| `python/pytest/asyncio/async-fixture-decorator` | `ast-grep` forbid: an `async def` decorated `@pytest.fixture` |

context.md: `@pytest_asyncio.fixture` for async fixtures; `loop_scope=` and `asyncio_default_fixture_loop_scope`, no `event_loop` fixture, `pytest_asyncio_loop_factories` instead of an `event_loop_policy` fixture.

### python/django

Detected by `django` (pypi) or a `manage.py` at the root or one level down.

| Rule | Targets |
|---|---|
| `python/django/makemigrations` | `check` migration: the shallowest tracked `manage.py`, `python <manage.py> makemigrations --check --dry-run --no-input` (no database needed: Django only warns when it cannot check the migration history); fails on "Migrations for", any other failure (settings that need an environment variable, a missing module) prints `skipped:` with the error's last line; `when_changed`: models, migrations, settings; timeout 120 s |
| `python/django/no-raw-sql-format` | `tool` ruff `S610` (`.extra()` with a non-literal `select` / `where` / `tables` / …), `S611` (`RawSQL()` with non-literal SQL); SQL strings built with an f-string, `.format()`, `%` or `+` (also through a variable) are `python/no-sql-format` (`S608`) |
| `python/django/flake8-django` | `tool` ruff `DJ001`, `DJ003`, `DJ006`, `DJ007`, `DJ013` |
| `python/django/csrf-exempt` | `ast-grep` forbid: `@csrf_exempt`, `csrf_exempt(…)`, `csrf_exempt` passed as an argument (`@method_decorator(csrf_exempt, …)`); the user decides, and disables the rule for that file in a local rule |
| `python/django/no-mark-safe` | `tool` ruff `S308` |
| `python/django/no-template-unescape` | `grep` advise on `@django-templates`: a `\|safe` or `\|safeseq` filter, `{% autoescape off %}` |
| `python/django/migrate-guard` | `guard` ask (`review`: is the user aware it resets data or the migration state): `manage.py` / `django-admin` `migrate … --fake`, `migrate <app> zero`, `flush`, `reset_db`, `sqlflush` piped to a command |
| `python/django/user-model-ref` | `ast-grep` forbid: a call whose callee is declared (§15.6) by `django` as `ForeignKey`, `OneToOneField` or `ManyToManyField` and whose first argument is the string `"auth.User"` or a name declared as `django.contrib.auth.models`' `User` |
| `python/django/async-orm` | `ast-grep` forbid, `needs` `django-stubs`: inside an `async def` (the innermost function; not a lambda or sync def in it), `$R.save/delete/refresh_from_db/full_clean/get/first/last/count/exists/create/get_or_create/update_or_create/update/aggregate/in_bulk/bulk_create/bulk_update/earliest/latest/contains/iterator(…)` and a sync `for … in $R`, where `R` is typed `django` `Model`, `QuerySet` or `BaseManager` (§15.1) |
| `python/django/queryset-len` | `ast-grep` advise, `needs` `django-stubs`: `len($Q)` where `Q` is a `QuerySet` (§15.1) |
| `python/django/no-atomic-in-async` | `ast-grep` forbid, `needs` `django-stubs`: a call typed `django.db.transaction` `Atomic` (§15.1) in a `with` item of an `async def`, or a decorator on an `async def` declared (§15.6) as `django.db.transaction`'s `atomic` (called or not) |
| `python/django/no-index-together` | `ast-grep` forbid, `needs` `django-stubs`: `index_together = …` in a `class Meta` of a class whose last base is typed `django` `Model` (§15.1); split from the candidate `no-deprecated-api` (another target) |
| `python/django/storages-setting` | `ast-grep` forbid in `@django-settings`, `when` `django >= 4.2`: top-level `DEFAULT_FILE_STORAGE = …` / `STATICFILES_STORAGE = …` (removed in 5.1, ignored); split from `no-deprecated-api` |
| `python/django/mailers-setting` | `ast-grep` forbid in `@django-settings`, `when` `django >= 6.1`: top-level `EMAIL_BACKEND`, `EMAIL_HOST`, `EMAIL_PORT`, `EMAIL_HOST_USER`, `EMAIL_HOST_PASSWORD`, `EMAIL_USE_TLS`, `EMAIL_USE_SSL`, `EMAIL_TIMEOUT`, `EMAIL_FILE_PATH`, `EMAIL_SSL_CERTFILE`, `EMAIL_SSL_KEYFILE` assignments (`MAILERS` replaces them); split from `no-deprecated-api` |
| `python/django/no-deprecated-api` | `ast-grep` forbid, `needs` `django-stubs`, `when` `django >= 6.1`: `$Q.select_related()` with no argument and `$Q.values_list(flat=True)` with no field, where `Q` is a `QuerySet` or `BaseManager` (§15.1); `CheckConstraint(check=)`, positional `save()`, `savepoint()` and `format_html()` without arguments are left to ty (`python/typecheck`, `python/no-deprecated`) |
| `python/django/migration-imports-models` | `ast-grep` forbid on `@django-migrations`: `from <x>.models import …`, `from <x> import models` (not `django.*`) |
| `python/django/migration-reversible` | `ast-grep` forbid on `@django-migrations`: `migrations.RunPython(…)` without a second positional argument or `reverse_code=`, `migrations.RunSQL(…)` without one or `reverse_sql=` |
| `python/django/applied-migrations` | `check` migration: `forge_applied_migrations @django-migrations`: a migration that exists in the base is not modified or deleted (not with `--all`) |

context.md: migrations generated and committed with model changes; SQL parameters; `select_related()` / `prefetch_related()` against N+1; `transaction.atomic()` and `transaction.on_commit()`; `format_html()` rather than `mark_safe()`; `@csrf_exempt` asked for.

context.md: migrations generated and committed with model changes; SQL parameters; `select_related()` / `prefetch_related()` against N+1; `transaction.atomic()` and `transaction.on_commit()`; on-commit callbacks tested with `django_capture_on_commit_callbacks(execute=True)` / `captureOnCommitCallbacks`, not a transactional test.

### python/django/drf

Detected by `djangorestframework` (pypi).

| Rule | Targets |
|---|---|
| `python/django/drf/permission-classes` | `check` lint: `forge_helper python drf_permissions.py` (§13.4): unless the settings module Django runs with (`DJANGO_SETTINGS_MODULE` set in `manage.py`, `wsgi.py` or `asgi.py`, star imports followed; else a `settings.py` / `settings/` outside tests) sets `DEFAULT_PERMISSION_CLASSES` to a non-empty list without `AllowAny`, a class inheriting a DRF view or viewset (directly or through project classes and mixins of any kind) gets a non-empty `permission_classes` or `get_permissions` from itself or a project ancestor, and an `@api_view` function has `@permission_classes`; `permission_classes = []` declares nothing; findings on the class or function line |
| `python/django/drf/serializer-fields` | `ast-grep` forbid: `fields = "__all__"` or `exclude = …` in the `Meta` of a class whose bases name a `Serializer` |
| `python/django/drf/no-csrf-exempt-session-auth` | `ast-grep` forbid: a class whose base is typed `rest_framework` `SessionAuthentication` (§15.1) and that defines `enforce_csrf` |
| `python/django/drf/no-allowany-write` | `ast-grep` forbid: `permission_classes` naming `AllowAny` in a `ModelViewSet`, a Create/Update/Destroy generic view or model mixin, or a class with `post`/`put`/`patch`/`delete`/`create`/`update`/`partial_update`/`destroy`; `@permission_classes([AllowAny])` on an `@api_view` with a write method; `AllowAny` as `DEFAULT_PERMISSION_CLASSES` |

context.md: explicit serializer fields and `read_only_fields`; permission defaults; ask before opening a write endpoint.

### python/django/ninja

Detected by `django-ninja` (pypi).

| Rule | Targets |
|---|---|
| `python/django/ninja/schema-fields` | `ast-grep` forbid: `fields = "__all__"` or `exclude = …` in the `Meta` of a class whose base is typed `ninja` `ModelSchema` (§15.1) |

### python/celery

Detected by `celery` (pypi).

| Rule | Targets |
|---|---|
| `python/celery/bounded-retries` | `ast-grep` forbid: `max_retries=None` in a `shared_task(…)` / `<x>.task(…)` call or a `.retry()`, `.apply_async()`, `.signature()`, `.set()` call; `"max_retries": None` inside `retry_kwargs=`; a `max_retries = None` class attribute of a class whose bases name `Task` |
| `python/celery/no-pickle` | `grep` forbid (case-insensitive): `task_serializer`, `result_serializer`, `event_serializer` or `serializer` set to `"pickle"` (`=` or `:`); `accept_content` naming `pickle` or `application/x-python-serialize` |
| `python/celery/no-result-get-in-task` | `ast-grep` forbid: inside a task, `.get()`, `.wait()`, `.join()` on `.delay()` / `.apply_async()` / `AsyncResult()` results (directly, through a variable of the function, or as the variable of a `for` loop or comprehension over them) or on a called `group` / `chord` / `chain`; `disable_sync_subtasks=False` anywhere |
| `python/celery/enqueue-on-commit` | `ast-grep` forbid, `needs` `celery-types` and `django-stubs`, `when` `django >= 4.2`: `$T.delay(…)` / `$T.apply_async(…)` (not in a `lambda`) inside a `with` whose item is typed `django.db.transaction` `Atomic` (§15.1), or in a function whose decorator is declared as its `atomic` (§15.6), where `T` is a Celery `Task` |
| `python/celery/no-model-args` | `ast-grep` forbid, `needs` `celery-types` and `django-stubs`, `when` `django >= 4.2`: an argument (positional or keyword) of `$T.delay/s/si(…)`, or an item of `$T.apply_async(args=[…], kwargs={…})`, typed `django` `Model` (§15.1), where `T` is a Celery `Task` |
| `python/celery/no-sqlalchemy-model-args` | the same, `needs` `celery-types`, `when` `sqlalchemy >= 2.0`, for an argument typed `sqlalchemy` `DeclarativeBase`; split from `no-model-args` (each needs its own stubs and version gate) |

context.md: ids as arguments, idempotent tasks queued from `transaction.on_commit()`, bounded retries, no waiting on results inside tasks, the json serializer.

### python/tortoise

Detected by `tortoise-orm` (pypi).

| Rule | Targets |
|---|---|
| `python/tortoise/primary-key-kw` | `ast-grep` forbid: a call typed `tortoise` (§15.1: a field constructor) with `pk=True` |
| `python/tortoise/no-generate-schemas` | `ast-grep` forbid, not in tests: a call whose callee is declared (§15.6) by `tortoise` as `generate_schemas`; a call with `generate_schemas=True` whose callee is declared by `tortoise` (`register_tortoise`, `RegisterTortoise`) |

### python/piccolo

Detected by `piccolo` (pypi).

| Rule | Targets |
|---|---|
| `python/piccolo/no-run-sync-in-async` | `ast-grep` forbid: `$Q.run_sync(…)` inside an `async def`, `$Q` typed `piccolo` |

### python/asyncpg

Detected by `asyncpg` (pypi).

| Rule | Targets |
|---|---|
| `python/asyncpg/pool-acquire-ctx` | `ast-grep` advise, `needs` `asyncpg-stubs`: `await $P.acquire(…)` where `P` is an asyncpg `Pool` (§15.1) |

### python/psycopg

Detected by `psycopg` or `psycopg-pool` (pypi).

| Rule | Targets |
|---|---|
| `python/psycopg/pool-open-explicit` | `ast-grep` forbid: a call typed `psycopg_pool` `ConnectionPool` / `AsyncConnectionPool` without `open=`, outside a `with` item |
| `python/psycopg/placeholder-style` | `ast-grep` advise: `execute()` / `executemany()` on a receiver typed `psycopg` `Connection`, `Cursor`, `ServerCursor` (and their async forms) with a string literal holding `$<digit>` and no dollar quote (`$$`, `$tag$`) |

context.md: `%s` placeholders; pools opened explicitly in startup.

### python/redis

Detected by `redis` or `aioredis` (pypi).

| Rule | Targets |
|---|---|
| `python/redis/no-keys` | `ast-grep` advise: `$R.keys(…)`, `$R` typed `redis` `Redis` / `StrictRedis` / `RedisCluster` (sync and `redis.asyncio`) |
| `python/redis/aclose` | `ast-grep` forbid: `$R.close()`, `$R` typed `redis.asyncio` `Redis` |
| `python/redis/no-aioredis` | `tool` ruff `TID251`, banned-api `aioredis` |

context.md: `scan_iter()` instead of `KEYS`; `aclose()` on asyncio clients.

### python/websockets

Detected by `websockets` (pypi).

| Rule | Targets |
|---|---|
| `python/websockets/no-legacy-asyncio` | `tool` ruff `TID251`, banned-api `websockets.legacy` |

### python/strawberry

Detected by `strawberry-graphql` (pypi).

| Rule | Targets |
|---|---|
| `python/strawberry/query-limits` | `ast-grep` advise: a call typed `strawberry` `Schema` without an `extensions=` argument naming `QueryDepthLimiter` |

### python/pandas

Detected by `pandas >= 3.0.0` (pypi). The receivers are typed `pandas` `DataFrame` / `Series` (§15.1); without pandas-stubs ty types the frame, so the rules ask for the innermost object (`df` in `df["a"][0]`).

| Rule | Targets |
|---|---|
| `python/pandas/no-chained-assignment` | `ast-grep` forbid: `$D[…][…] = …` (and `+=`, `-=`), `$D.loc[…][…] = …`, `$D[…].iloc[…] = …` (`loc`, `iloc`, `at`, `iat`) |
| `python/pandas/no-chained-inplace` | `ast-grep` forbid: `$D[…].$M(…, inplace=True)`, `$D.loc[…].$M(…, inplace=True)` (`loc`, `iloc`) |
| `python/pandas/no-deprecated-inplace` | `ast-grep` forbid: `inplace=True` on `rename`, `rename_axis`, `reset_index`, `set_index`, `drop`, `drop_duplicates`, `dropna`, `sort_values`, `sort_index`, `query`, `eval` (deprecated in pandas 3.1) |
| `python/pandas/no-object-dtype-check` | `ast-grep` forbid: `.dtype` / `.dtypes` compared (`==`, `!=`) with `object`, `"object"`, `"O"` or `<module>.object_`; `select_dtypes(…)` naming `object` / `"O"` |
| `python/pandas/no-removed-freq-aliases` | `ast-grep` forbid: a call typed `pandas` whose `freq=` / `rule=` or first argument of `resample` / `asfreq` / `rolling` / `to_period` is a removed alias (`BM`, `SM`, `CBM`, `BQ`, `BY`, `A`, `AS`, `BA`, `BAS`, `H`, `BH`, `CBH`, `T`, `L`, `U`, `N`, `S`, with a multiple and an anchor), or `M` / `Q` / `Y` outside `period_range`, `to_period`, `Period`, `PeriodIndex` |
| `python/pandas/no-row-loops` | `ast-grep` advise: `$R.iterrows()`, `$R.itertuples(…)`, `$R.apply(…, axis=1 / "columns")` on a `DataFrame` |

context.md: pandas 3 Copy-on-Write (one indexer per assignment, results assigned), the `str` dtype, the new frequency aliases, vectorized operations.

### python/polars

Detected by `polars` (pypi).

| Rule | Targets |
|---|---|
| `python/polars/no-python-row-functions` | `ast-grep` advise: `map_elements`, `map_rows`, `iter_rows` on a receiver typed `polars` `Expr` / `Series` / `DataFrame` |

### python/pandera

Detected by `pandera` (pypi).

| Rule | Targets |
|---|---|
| `python/pandera/pandas-namespace` | `tool` ruff `TID251`, banned-api `pandera.` `DataFrameSchema`, `SeriesSchema`, `Column`, `Index`, `MultiIndex`, `DataFrameModel`, `infer_schema`, `check_types`, `check_input`, `check_output`, `check_io` (not `Check`, `Field`, `check`, `dataframe_check`, `parser`: pandera keeps them top-level) |

### python/numpy

Detected by `numpy` (pypi).

| Rule | Targets |
|---|---|
| `python/numpy/legacy-random` | `tool` ruff `NPY002` |
| `python/numpy/no-allow-pickle` | `ast-grep` forbid: a call with `allow_pickle=True` whose callee is declared (§15.6) by `numpy` as `load` (`np.save(allow_pickle=True)` is not matched) |

### python/torch

Detected by `torch` (pypi).

| Rule | Targets |
|---|---|
| `python/torch/no-unsafe-load` | `ast-grep` forbid: the keyword argument `weights_only=False` |
| `python/torch/no-unsafe-load-env` | `grep` forbid: `TORCH_FORCE_NO_WEIGHTS_ONLY_LOAD` in `*.py`, `.env` files, Dockerfiles, Compose files, `*.sh`, `*.toml`, `*.ini`, `*.cfg`, `.github/workflows/*` |
| `python/torch/save-state-dict` | `ast-grep` advise: a call whose callee is declared (§15.6) in `torch.serialization` as `save` and whose first argument's type is (a subclass of) `torch`'s `Module` (§15.1); `torch.jit.save` is another declaration |

### python/huggingface

Detected by `transformers`, `datasets`, `huggingface-hub`, `sentence-transformers`, `diffusers`, `peft` or `vllm` (pypi).

| Rule | Targets |
|---|---|
| `python/huggingface/no-trust-remote-code` | `ast-grep` advise: the keyword argument `trust_remote_code=True` |
| `python/huggingface/no-pickle-weights` | `ast-grep` forbid: the keyword argument `use_safetensors=False` |

### python/jupyter

Detected by a `*.ipynb` file.

| Rule | Targets |
|---|---|
| `python/jupyter/no-outputs` | `grep` forbid on `**/*.ipynb`: `"output_type": "…"`, `"execution_count": <number>` |

context.md: outputs cleared before committing (nbstripout as a git filter).

### python/anthropic

Detected by `anthropic >= 2.0.0` (pypi).

| Rule | Targets |
|---|---|
| `python/anthropic/no-version-header` | `ast-grep` advise: a call typed `anthropic` with `default_headers=` / `extra_headers=` holding an `"anthropic-version"` key |
| `python/anthropic/stream-long-requests` | `ast-grep` advise: `create(…)` / `parse(…)` on a receiver typed `anthropic` `Messages` / `AsyncMessages` with an integer `max_tokens` above 21,333 and no `stream=True` |
| `python/anthropic/no-budget-tokens` | `ast-grep` forbid: `$C.$M(…)` (`create`, `stream`, `parse`, `count_tokens`) on a receiver typed `anthropic` `Messages` / `AsyncMessages` with a `model=` string literal from the data file's `rejects budget_tokens` rows (`ai-models`; optionally `anthropic.`-prefixed) and a `thinking=` dict holding a `budget_tokens` key |

### python/langchain

Detected by `langchain-core` (pypi).

| Rule | Targets |
|---|---|
| `python/langchain/no-dangerous-flags` | `ast-grep` advise: the keyword arguments `allow_dangerous_deserialization=True`, `allow_dangerous_code=True`, `allow_dangerous_requests=True` |
| `python/langchain/safe-load` | `ast-grep` forbid: the keyword arguments `allowed_objects="all"`, `secrets_from_env=True` |
| `python/langchain/partner-packages` | `tool` ruff `TID251`, banned-api: each `langchain_community` class deprecated with an `alternative_import` in a partner package (langchain-community 0.4.2: 84 classes, 164 entries), at its defining module and at its package's re-export (TID251 matches a path, not the object) |
| `python/langchain/no-legacy-chains` | `tool` ruff `TID251`, banned-api `langchain_classic` |
| `python/langchain/no-python-repl` | `tool` ruff `TID251`, banned-api `langchain_experimental` `PythonREPLTool`, `PythonAstREPLTool` (`tools`, `tools.python.tool`), `PythonREPL` (`utilities`, `utilities.python`) |

### python/llamaindex

Detected by `llama-index-core` (pypi).

| Rule | Targets |
|---|---|
| `python/llamaindex/no-service-context` | `tool` ruff `TID251`, banned-api `llama_index.core.ServiceContext`, `llama_index.core.service_context.ServiceContext`, `llama_index.core.set_global_service_context` |
| `python/llamaindex/no-experimental` | `tool` ruff `TID251`, banned-api `llama_index.experimental` |

### python/mcp

Detected by `mcp` (pypi).

| Rule | Targets |
|---|---|
| `python/mcp/dns-rebinding-protection` | `ast-grep` forbid: a call typed `mcp` `TransportSecuritySettings` with `enable_dns_rebinding_protection=False` |

### python/opentelemetry

Detected by `opentelemetry-sdk` (pypi).

| Rule | Targets |
|---|---|
| `python/opentelemetry/batch-processor` | `ast-grep` advise: a call typed `opentelemetry` `SimpleSpanProcessor` / `SimpleLogRecordProcessor` whose argument is typed `opentelemetry.exporter.otlp` `OTLPSpanExporter` / `OTLPLogExporter` |

### python/airflow

Detected by `apache-airflow` (pypi).

| Rule | Targets |
|---|---|
| `python/airflow/dag-schedule` | `tool` ruff `AIR002` |
| `python/airflow/airflow3` | `tool` ruff `AIR301`, `AIR302`, `AIR303`, `AIR311`, `AIR312` (stable; not the preview `AIR304`, `AIR321`); `tool` ruff `TID251`, banned-api `airflow.settings.Session`, `airflow.utils.session.provide_session`, `airflow.utils.session.create_session`; only with `apache-airflow >= 3.0.0` (`when`) |

### python/prefect

Detected by `prefect` (pypi).

| Rule | Targets |
|---|---|
| `python/prefect/no-removed-v2-api` | `tool` ruff `TID251`, banned-api: Prefect's own migration table (`prefect/_internal/compatibility/migration.py`, 3.9.0: `REMOVED_IN_V3` and `MOVED_IN_V3`, `a.b:C` as `a.b.C`), the message naming Prefect's replacement; only with `prefect >= 3.0.0` (`when`) |

### python/sentry

Detected by `sentry-sdk` (pypi).

| Rule | Targets |
|---|---|
| `python/sentry/span-attributes` | `ast-grep` forbid: a call with `data=` whose callee is declared (§15.6) by `sentry_sdk` as `update_current_span` |
| `python/sentry/no-deprecated-scope-api` | `tool` ruff `TID251`, banned-api `sentry_sdk.configure_scope`, `push_scope`, `Hub`, `set_measurement` (each also at its defining module, `sentry_sdk.api` / `sentry_sdk.hub`); only with `sentry-sdk >= 2.0.0` (`when`) |

### python/temporal

Detected by `temporalio` (pypi).

| Rule | Targets |
|---|---|
| `python/temporal/workflow-determinism` | `ast-grep` forbid, inside a class decorated with a name declared (§15.6) in `temporalio.workflow` as `defn` (bare or called): a call declared by `datetime` as `now` / `utcnow` / `today`, by `time` as `time`, `time_ns`, `monotonic`, `monotonic_ns`, `perf_counter`, `perf_counter_ns`, `sleep`, by `random`, by `uuid` as `uuid1` / `uuid4`, by `os` as `getenv`, by `builtins` as `open`, by `threading`, by `asyncio` as `to_thread`, `wait`, `as_completed`, `run_in_executor`, or by `httpx`, `requests`, `aiohttp`; an attribute declared by `os` as `environ` |
| `python/temporal/activity-timeout` | `ast-grep` forbid: a call without `start_to_close_timeout=` or `schedule_to_close_timeout=` whose callee is declared (§15.6) in `temporalio.workflow` as `execute_activity`, `start_activity`, `execute_local_activity`, `start_local_activity` or their `_method` / `_class` variants |
| `python/temporal/no-sandbox-escape` | `ast-grep` forbid: a call with `sandboxed=False` whose callee is declared (§15.6) in `temporalio.workflow` as `defn`; a call declared by `temporalio` as `UnsandboxedWorkflowRunner` or `sandbox_unrestricted` |

### python/library

Detected by `library_any: [pypi]` (§13.4).

| Rule | Targets |
|---|---|
| `python/library/all-exports` | `ast-grep` no-increase, not in tests: in a module with `__all__`, top-level function and class names without a leading underscore that `__all__` does not list |
| `python/library/deprecation-shim` | `check` lint: `forge_helper python public_api.py` (§13.4), `when_changed`: `**/*.py`, `pyproject.toml` |
| `python/library/spdx-license` | `grep` forbid in `pyproject.toml`: `license = {…}` (a table), `"License ::` classifiers (PEP 639) |
| `python/library/no-python-cap` | `grep` forbid in `pyproject.toml`: `requires-python` with `<`, `~=` or `==` |
| `python/library/no-logging-config` | `ast-grep` forbid, not in tests or `__main__.py`: `logging.basicConfig(…)`, `logging.getLogger().addHandler(…)`, `logging.root.addHandler(…)` |
| `python/library/py-typed` | `check` lint, `when_changed`: `pyproject.toml`, `**/py.typed`: `uv build --wheel` into the check's temporary directory, then each top-level package of the wheel (a directory with `.py` files, not `.dist-info` / `.data`) holds `py.typed` (PEP 561); a failed build prints `skipped` with its last line |

context.md: `__all__` is the public API; removals and renames warn with `DeprecationWarning` until a major version; SPDX `license` (and the backends that support it), no `requires-python` cap; no logging configuration in library code.

### python/pyjwt

Detected by `pyjwt` (pypi).

| Rule | Targets |
|---|---|
| `python/pyjwt/jwt-verify` | `ast-grep` forbid: `jwt.decode(…)`, or `$J.decode(…)` where `J` is typed `jwt` `PyJWT` / `PyJWS` (§15.1), without `algorithms` (keyword or third positional argument; a `**kwargs` is not judged), or with `verify=False`, `options={…"verify_signature": False…}` or `"none"` (any case) in the algorithms list |

context.md: `algorithms=[…]` always, never `"none"`; no signature check turned off; `jwt.get_unverified_header()` for an unverified header.

### python/typer

Detected by `typer` (pypi).

| Rule | Targets |
|---|---|
| `python/typer/annotated-params` | `ast-grep` forbid: a parameter whose default value is a `typer.Option(…)` / `typer.Argument(…)` call (or `Option(…)` / `Argument(…)` imported from `typer`) |

context.md: parameters declared with `Annotated[T, typer.Option(...)]`, the default as the parameter's default.

### python/structlog

Detected by `structlog` (pypi).

| Rule | Targets |
|---|---|
| `python/structlog/constant-event` | `ast-grep` forbid: `$L.<level>(…)` (`debug`, `info`, `warning`, `warn`, `error`, `err`, `exception`, `critical`, `fatal`, `msg`, and their `a`-prefixed async forms) whose first argument is an f-string, a `.format()` call or a `%`-formatted string, where `L` is typed `structlog` `BoundLoggerBase` / `BindableLogger` (§15.1) |
| `python/structlog/typed-logger` | `ast-grep` advise: `structlog.get_logger(…)` (typed `Any`) not assigned to an annotated name |

context.md: constant events with key/value data; annotate loggers from `structlog.get_logger()`.

### python/msgspec

Detected by `msgspec` (pypi).

| Rule | Targets |
|---|---|
| `python/msgspec/typed-decode` | `ast-grep` forbid: `msgspec.json` / `msgpack` / `yaml` / `toml` `.decode(…)` without `type=` (a `from msgspec import json` alias is not matched) |

context.md: decode with `type=` or a typed `Decoder`.

### python/loguru

Detected by `loguru` (pypi).

| Rule | Targets |
|---|---|
| `python/loguru/no-fstring-with-args` | `ast-grep` forbid: `$L.<level>(…)` (`trace`, `debug`, `info`, `success`, `warning`, `error`, `exception`, `critical`, `log`) whose first argument is an f-string and that has a second argument, where `L` is typed `loguru` `Logger` (§15.1) |

context.md: an f-string or a format string with arguments, never both.

### python/uvicorn

Detected by `uvicorn` (pypi).

| Rule | Targets |
|---|---|
| `python/uvicorn/uvicorn-worker` | `grep` forbid on `@dockerfile`, `@procfile`, `@compose`, `@gunicorn-config`, `**/*.py`: `uvicorn.workers.UvicornWorker` / `UvicornH11Worker` (deprecated; `uvicorn_worker.UvicornWorker`) |
| `python/uvicorn/no-legacy-ws-impl` | `grep` forbid on `@dockerfile`, `@procfile`, `@compose`, `**/*.py`: `--ws websockets`, `ws="websockets"`, `UVICORN_WS=websockets` (the deprecated legacy implementation) |
| `python/uvicorn/forwarded-allow-all` | `grep` forbid (case-insensitive) on `@dockerfile`, `@procfile`, `@compose`, `@env-files`, `@gunicorn-config`, `**/*.py`: `--forwarded-allow-ips "*"`, `forwarded_allow_ips = "*"`, `FORWARDED_ALLOW_IPS=*` |
