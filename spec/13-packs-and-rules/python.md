<!-- Generated from the built-in packs by crates/forge-packs/tests/spec_tables.rs; do not edit. Regenerate: FORGE_BLESS=1 cargo test -p forge-packs --test spec_tables -->

## 13. Packs and rules: `python`

### python

Python projects, type-checked with ty.

Detected by: a file matching `pyproject.toml`, `setup.py`, `setup.cfg`, `requirements*.txt`, `Pipfile`; or a source file of the project's own matching `**/*.py`.

Context (in `.claude/rules/forge-*.md`, §7):

- Parameters and return types are annotated.
- Inside `async def`, no blocking calls: sync HTTP clients, `time.sleep`, `open()` and `pathlib` I/O, blocking `subprocess`, `input()`. A caught `asyncio.CancelledError` is re-raised, and a `try` around a task group catches with `except*`.
- Ignore comments name their rule (`# ty: ignore[rule-name]`), and a changed file does not gain suppression comments.
- (when `python.pm` is `none`) This project has no Python packaging file: when a third-party package is needed, create the project with `uv init --bare`, then `uv add <pkg>`.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `python/annotations-api` | Read annotations with annotationlib.get_annotations() or inspect.get_annotations(), not __dict__["__annotations__"] (empty under lazy annotations). | python | tool ruff `RUF063` |
| `python/archive-extract` | TarFile.extractall() / extract() and shutil.unpack_archive() pass filter= (normally "data"): without it a crafted archive writes outside the target directory (absolute paths, .., links). zipfile sanitises its paths itself. | python | ast-grep forbid (where `$T`, `$U`: types engine, §15) |
| `python/aware-datetime` | Datetimes carry a time zone: datetime.now(UTC), datetime(..., tzinfo=UTC), fromtimestamp(ts, tz=UTC); no datetime.today(), date.today() or naive datetime.min/max. Naive datetimes shift silently against timestamptz columns. | python | tool ruff `DTZ005`<br>tool ruff `DTZ001`<br>tool ruff `DTZ002`<br>tool ruff `DTZ006`<br>tool ruff `DTZ011`<br>tool ruff `DTZ012`<br>tool ruff `DTZ901` |
| `python/config-files` | Claude asks you before editing a dedicated Python tool config (ruff, ty, mypy, pyright); pyproject.toml is covered by python/config-ratchet. | any | permission ask `Edit(**/ruff.toml)`<br>permission ask `Edit(**/.ruff.toml)`<br>permission ask `Edit(**/ty.toml)`<br>permission ask `Edit(**/mypy.ini)`<br>permission ask `Edit(**/.mypy.ini)`<br>permission ask `Edit(**/pyrightconfig.json)` |
| `python/config-ratchet` | Python tool configs are not loosened (ruff rules dropped or ignored, ty rule levels lowered, excludes added, pytest paths or coverage thresholds reduced, mypy strictness turned off or errors ignored, pyright checking mode or report levels lowered); loosening one is the user's decision: ask them. | any: `**/pyproject.toml`, `**/ruff.toml`, `**/.ruff.toml`, `**/ty.toml`, `**/mypy.ini`, `**/.mypy.ini`, `**/pyrightconfig.json` | check lint: `forge_ratchet python ratchet.py` (when `**/pyproject.toml`, `**/ruff.toml`, `**/.ruff.toml`, `**/ty.toml`, `**/mypy.ini`, `**/.mypy.ini` or `**/pyrightconfig.json` changes) |
| `python/contextmanager-finally` | In a @contextmanager / @asynccontextmanager function, a yield followed by cleanup sits in try/finally (or inside a with): otherwise an exception in the with body skips the cleanup. | python | ast-grep forbid |
| `python/decimal-from-str` | Decimal is built from a string or an int (Decimal("0.1")), not from a float literal (Decimal(0.1) is 0.1000000000000000055...). | python | tool ruff `RUF032` |
| `python/env-example` | A pydantic-settings field added to a BaseSettings class is listed in the .env template next to it (.env.example, .env.template, .env.sample, .env.dist, .env.<name>.example): always when it has no default, and also with a default when the template already lists at least half of the class's variables. Checked at Stop and pre-commit. | python | check lint: `forge_env_example py` |
| `python/env-misuse` | No assignment to os.environ (it is a mapping: set keys), os.getenv defaults are strings, and os.environ is copied with .copy(), not copy.copy(). | python | tool ruff `B003`<br>tool ruff `PLW1508`<br>tool ruff `PLW1507` |
| `python/format` | Python files are formatted with ruff. | python | check format: `forge_do ruff format-check .` |
| `python/generic-type-args` | Generic classes are parameterised in annotations (list[str], re.Match[str]), never bare. | python | tool ty `missing-type-argument` |
| `python/hmac-compare` | An HMAC digest is compared with hmac.compare_digest(), never == or !=: == stops at the first differing byte, so its timing lets an attacker forge a webhook signature byte by byte. | python | ast-grep forbid (where `$H`, `$D`: types engine, §15) |
| `python/http-timeout` | HTTP calls never wait forever: requests calls set a timeout, httpx calls never pass timeout=None. | python | tool ruff `S113` |
| `python/jinja-autoescape` | Jinja2 environments escape HTML (autoescape on). | python | tool ruff `S701` |
| `python/lazy-log-args` | A logging Logger or LoggerAdapter call (debug, info, warning, error, exception, critical, and log's message) passes a constant format string and its arguments (log.info("user %s", uid)), not an f-string, .format() or %-formatted string: the message would be built even when the level is off, and log tools could not group it. | python | ast-grep forbid (where `$L`: types engine, §15) |
| `python/lint` | Python files pass ruff check. | python | check lint: `forge_do ruff lint .` (safe fixes after an edit) |
| `python/llm-client-timeout` | Anthropic and OpenAI SDK clients and calls never set timeout=None (the client constructor, with_options(), a request): a request could then wait forever, and is retried. Keep the SDK's default or pass a number of seconds. | python | ast-grep forbid (where `$CALL`: types engine, §15) |
| `python/log-exception` | Inside except, a logging Logger or LoggerAdapter logs the failure with .exception(…) (or passes exc_info=), not .error(…): .error drops the traceback. | python | ast-grep forbid (where `$L`: types engine, §15) |
| `python/loop-closure` | Closures and lambdas defined in a loop do not capture the loop variable (it is read when they run, not when they are made): bind it as a default argument or with functools.partial. | python | tool ruff `B023` |
| `python/misplaced-bare-raise` | A bare raise is only used inside an exception handler. | python | tool ruff `PLE0704` |
| `python/no-async-timeout` | Use asyncio.timeout(), not the async_timeout package. | python | tool ruff `TID251` |
| `python/no-bare-except` | No bare except. Catch the exceptions you expect. | python | tool ruff `E722` |
| `python/no-bidi-chars` | No bidirectional-control or zero-width-space characters in source (they make code read differently from how it runs). | python | tool ruff `PLE2502`<br>tool ruff `PLE2515` |
| `python/no-blanket-noqa` | A noqa comment names the codes it silences. | python | tool ruff `PGH004` |
| `python/no-blind-except` | No except Exception (or BaseException) unless it re-raises or logs with logger.exception. | python | tool ruff `BLE001` |
| `python/no-blocking-in-async` | Async functions do not block the event loop (sync HTTP clients, sleep, file and pathlib I/O, blocking subprocesses, input()) or misuse async primitives (a timeout scope without an await, an unawaited trio call, a sleep-polling loop instead of an Event). | python | tool ruff `ASYNC210`<br>tool ruff `ASYNC230`<br>tool ruff `ASYNC251`<br>tool ruff `ASYNC212`<br>tool ruff `ASYNC220`<br>tool ruff `ASYNC221`<br>tool ruff `ASYNC222`<br>tool ruff `ASYNC240`<br>tool ruff `ASYNC250`<br>tool ruff `ASYNC100`<br>tool ruff `ASYNC105`<br>tool ruff `ASYNC110` |
| `python/no-cached-method` | No functools.lru_cache or cache on methods: the cache keeps every instance alive; cache a module-level function or use cached_property. | python | tool ruff `B019` |
| `python/no-commented-out-code` | No commented-out code. Delete it; git keeps history. | python | tool ruff `ERA001` |
| `python/no-commonprefix` | Use os.path.commonpath, not os.path.commonprefix, for paths: commonprefix compares characters (/usr/lib and /usr/local give /usr/l). | python | tool ruff `RUF071` |
| `python/no-dangling-task` | asyncio.create_task results are kept: an unreferenced task can be garbage-collected mid-flight. | python | tool ruff `RUF006` |
| `python/no-debugger` | No breakpoint(), pdb.set_trace() or other debugger hooks in committed code. | python | tool ruff `T100` |
| `python/no-deprecated` | No use of APIs their library marks deprecated (PEP 702, typeshed). Use the replacement the warning names. | python | tool ty `deprecated` |
| `python/no-dev-server-in-image` | Dockerfile CMD / ENTRYPOINT and Procfile processes run no development server: no --reload, fastapi dev or flask run --debug (a reloader wastes CPU in production, and Flask's debugger runs code from the browser). Use fastapi run, uvicorn without --reload, flask run without --debug, or gunicorn. | any: `@dockerfile`, `@procfile` | grep forbid `^[[:space:]]*(CMD\|ENTRYPOINT\|[A-Za-z0-9_-]+:).*(--reload([^-A-Za-z]\|$)\|fastapi["',[:space:]]+dev([^A-Za-z]\|$)\|flask["',[:space:]]+(.*["',[:space:]])?--debug)` |
| `python/no-ensure-future` | Schedule coroutines with asyncio.create_task() or a TaskGroup, not asyncio.ensure_future(). | python | tool ruff `TID251` |
| `python/no-eval` | No eval() or exec(): parse data with ast.literal_eval or json. | python | tool ruff `S307`<br>tool ruff `S102` |
| `python/no-explicit-any` | No explicit Any. Use a precise type, a Protocol, or object with narrowing. | python | tool ruff `ANN401` |
| `python/no-forgotten-call` | No truthiness test of a method, a callable or an iterable; call the method, compare with None, or use a sequence. | python | tool ty `redundant-condition`<br>tool ty `truthiness-test-of-callable`<br>tool ty `truthiness-test-of-iterable` |
| `python/no-get-event-loop` | No asyncio.get_event_loop(); use asyncio.run() at the entry point and asyncio.get_running_loop() inside coroutines. | python | tool ruff `TID251` |
| `python/no-global-statement` | No global statements: pass state explicitly or keep it in an object. | python | tool ruff `PLW0603` |
| `python/no-hardcoded-password` | No password or secret literal in code. Read it from the environment or a secret store. | python | tool ruff `S105`<br>tool ruff `S106` |
| `python/no-jump-in-finally` | No return, break or continue inside finally: it swallows the exception in flight. | python | tool ruff `B012` |
| `python/no-mutable-defaults` | No mutable defaults: function arguments (B006), ContextVar defaults (B039), dataclass fields (RUF008) and no function call as a dataclass default (RUF009); __post_init__ parameters take no defaults (RUF033: put them on the InitVar field). | python | tool ruff `B006`<br>tool ruff `B039`<br>tool ruff `RUF008`<br>tool ruff `RUF009`<br>tool ruff `RUF033` |
| `python/no-nest-asyncio` | No nest_asyncio (archived); call async code with await, or run it in a thread. | python | tool ruff `TID251` |
| `python/no-new-suppressions` | No new suppression comments (# noqa, # ty: ignore, # type: ignore, # pyright: ignore) or coverage pragmas (# pragma: no cover, # pragma: no branch); a changed file may not have more than it had. Fix the finding instead. | python | ast-grep no-increase |
| `python/no-passlib` | No passlib, which is unmaintained; use pwdlib, argon2-cffi or bcrypt. | python | tool ruff `TID251` |
| `python/no-pickle` | No pickle or marshal on data you did not write: unpickling runs code, and marshal is not safe against crafted input. Use JSON or a schema. | python | tool ruff `S301`<br>tool ruff `S302` |
| `python/no-pkg-resources` | Use importlib.metadata and importlib.resources, not pkg_resources. | python | tool ruff `TID251` |
| `python/no-possibly-missing-submodule` | Import the submodule you use (import xml.etree.ElementTree), not only its parent package. | python | tool ty `possibly-missing-submodule` |
| `python/no-preexec-fn` | subprocess does not take preexec_fn: it is not safe with threads; use start_new_session, process_group, user or group. | python | tool ruff `PLW1509` |
| `python/no-print` | No print in application code. Use logging. | python | tool ruff `T201` |
| `python/no-python-jose` | No python-jose (jose), which is unmaintained; use PyJWT or joserfc. | python | tool ruff `TID251` |
| `python/no-pytz` | Use zoneinfo.ZoneInfo, not pytz. | python | tool ruff `TID251` |
| `python/no-raise-vanilla` | No raise Exception(...) or BaseException(...): raise a specific built-in or project exception class that callers can catch. | python | tool ruff `TRY002` |
| `python/no-removed-stdlib` | No standard library modules removed in Python 3.12 and 3.13 (distutils, imp, asyncore, cgi, crypt, imghdr, pipes, telnetlib, PEP 594). | python | tool ruff `TID251` |
| `python/no-root-logger` | Log through logging.getLogger(__name__): no calls on the root logger (logging.info(...)), no logging.Logger(...) construction, no getLogger(__file__). | python | tool ruff `LOG015`<br>tool ruff `LOG001`<br>tool ruff `LOG002` |
| `python/no-ruff-overlap` | No black, isort, flake8, autopep8, pyupgrade or autoflake as a dependency or pre-commit hook: ruff format and ruff check cover them, and two tools fight over the same lines. | any: `**/pyproject.toml`, `.pre-commit-config.yaml` | grep forbid `"(black\|isort\|flake8\|autopep8\|pyupgrade\|autoflake)([[:space:]]*[]<>=!~;[]\|")\|repo:[[:space:]]*https://github\.com/(psf/black(-pre-commit-mirror)?\|pycqa/(isort\|flake8\|autoflake)\|asottile/pyupgrade\|hhatto/autopep8)([.]git)?[[:space:]]*$` (ignoring case) |
| `python/no-setup-py` | Claude asks you before creating setup.py or setup.cfg; project metadata and the build backend live in pyproject.toml. | any | permission ask `Write(**/setup.py)`<br>permission ask `Write(**/setup.cfg)` |
| `python/no-shell-true` | Commands run as an argument list, never through a shell (subprocess shell=True, os.system, os.popen). | python | tool ruff `S602`<br>tool ruff `S605` |
| `python/no-silent-except` | No except that only passes or continues: handle the error, log it, or let it propagate. | python | tool ruff `S110`<br>tool ruff `S112` |
| `python/no-sql-format` | No SQL built with f-strings, format or concatenation. Use bound parameters. | python | tool ruff `S608` |
| `python/no-ssl-verify-off` | No check_hostname = False or verify_mode = CERT_NONE on an ssl SSLContext: it turns off certificate and hostname verification for every connection made with the context. | python | ast-grep forbid (where `$C`, `$V`: types engine, §15) |
| `python/no-stale-ignore` | No ignore comment that silences nothing (ty: ignore or type: ignore); remove it once the code it covered is fixed. | python | tool ty `unused-ignore-comment`<br>tool ty `unused-type-ignore-comment` |
| `python/no-stub` | No stub left behind. A function whose body is only pass or ..., or that raises NotImplementedError, is a stub unless it is abstract, an overload or part of a Protocol (stub files and migrations are exempt). | python: `!**/*.pyi`, `!@alembic-migrations`, `!@django-migrations` | ast-grep forbid |
| `python/no-swallowed-cancel` | An except handler that catches asyncio.CancelledError (alone or in a tuple) re-raises it: swallowing it breaks cancellation, timeouts and task group shutdown. | python | ast-grep forbid |
| `python/no-tls-bypass` | TLS certificate checks stay on (no verify=False, no ssl._create_unverified_context()) and no insecure protocol version is requested (ssl_version= / method= SSLv2, SSLv3, TLSv1, TLSv1.1). | python | tool ruff `S501`<br>tool ruff `S323`<br>tool ruff `S502` |
| `python/no-unawaited-coroutine` | Every coroutine is awaited or handed to a task API; a call whose awaitable is dropped never runs. | python | tool ty `unused-awaitable` |
| `python/no-unknown` | Parameters and return types are annotated (special, static and class methods included), so no value is Unknown to the type checker (and type constraints of other rules see every receiver). | python | tool ruff `ANN001`<br>tool ruff `ANN002`<br>tool ruff `ANN003`<br>tool ruff `ANN201`<br>tool ruff `ANN202`<br>tool ruff `ANN204`<br>tool ruff `ANN205`<br>tool ruff `ANN206` |
| `python/no-unsafe-markup` | No markupsafe.Markup() around a string built at run time (f-string, .format(), +): it marks unescaped input safe. Escape the values with escape() or Markup.format(). | python | tool ruff `S704` |
| `python/no-unsafe-yaml` | yaml.load needs a safe loader; use yaml.safe_load. | python | tool ruff `S506` |
| `python/no-unscoped-ignore` | Ignore comments name the rule they silence (# ty: ignore[rule]); no blanket ignores. | python | tool ty `blanket-ignore-comment`<br>tool ruff `PGH003` |
| `python/no-unused-args` | No unused function or method arguments (methods marked @override and abstract methods are exempt; prefix an intentionally unused one with an underscore). | python | tool ruff `ARG001`<br>tool ruff `ARG002` |
| `python/no-unused-noqa` | No noqa comment that silences nothing. | python | tool ruff `RUF100` |
| `python/no-useless-cast` | No cast() that is unnecessary or between unrelated types. Narrow with isinstance or validate instead. | python | tool ty `redundant-cast`<br>tool ty `disjoint-cast` |
| `python/no-useless-lock` | A lock is shared, not created in its own with statement (with threading.Lock(): protects nothing). | python | tool ruff `PLW2101` |
| `python/no-useless-try` | No except handler that only re-raises (drop the try), and re-raise with a bare raise, not raise err. | python | tool ruff `TRY203`<br>tool ruff `TRY201` |
| `python/no-weak-hash` | No MD5 or SHA-1 for security. Use SHA-256 or better (or usedforsecurity=False for checksums). | python | tool ruff `S324` |
| `python/open-with-context` | Files are opened in a with block, so they are closed on every path. | python | tool ruff `SIM115` |
| `python/override-decorator` | A method overriding a base-class method carries @typing.override. | python | tool ty `missing-override-decorator` |
| `python/pip-install` | Packages go into the project through its package manager, not with pip into its environment. | any | guard deny: check `python-pm` |
| `python/project-table` | pyproject.toml declares the project in a PEP 621 [project] table with requires-python, not only in tool-specific metadata such as [tool.poetry]. | any: `pyproject.toml` | check lint: a shell script calling `forge_toml_get` (when `pyproject.toml` changes) |
| `python/raise-from` | Inside except, a new exception is raised from the caught one (raise X from err) or explicitly from None, so the cause is kept. | python | tool ruff `B904` |
| `python/ruff-config-current` | Ruff settings use their current names (linter options under [tool.ruff.lint] / [lint], no deprecated option): ruff prints no deprecation warning for the project's config. | any: `**/pyproject.toml`, `**/ruff.toml`, `**/.ruff.toml` | check lint: `! forge_do ruff settings-warnings . 2>&1 >/dev/null \| grep -iE -A5 '^warning: .*deprecat'` (when `**/pyproject.toml`, `**/ruff.toml` or `**/.ruff.toml` changes) |
| `python/self-type` | Methods returning their own class use typing.Self, not a TypeVar bound to the class; __enter__, __new__ and in-place operators return Self. | python | tool ruff `PYI019`<br>tool ruff `PYI034` |
| `python/strict-zip` | zip() passes strict= (strict=True when the iterables must have the same length), so a length mismatch is not silently truncated. | python | tool ruff `B905` |
| `python/stub-sync` | A public name a turn adds to a module (in __all__, or a top-level def or class) also goes into each .pyi stub that mirrors that module elsewhere in the repository; advisory, it blocks Stop once per turn. | python: `**/*.py` | check lint: `forge_advise forge_stub_sync`<br>advice, never blocks |
| `python/subprocess-check` | subprocess.run() states check=True (fail on a non-zero exit) or check=False (the caller reads returncode). | python | tool ruff `PLW1510` |
| `python/taskgroup-except-star` | A try whose body is one task group (asyncio.TaskGroup, anyio task group, trio nursery) catches with except*, ExceptionGroup or Exception: a task group raises an ExceptionGroup, which a plain except SomeError never catches. | python | ast-grep forbid (where `$CM`: types engine, §15) |
| `python/type-checking-imports` | A name imported only under if TYPE_CHECKING: is not used at run time (it would raise NameError). | python | tool ruff `TC004` |
| `python/typecheck` | The project passes ty, with the project's environment and its [tool.ty] config. | python | check typecheck: `forge_do ty typecheck` |
| `python/typed-decorators` | Decorators keep the decorated function's type; none returns Unknown or Any. | python | tool ty `dynamic-function-decorator-return` |
| `python/typeis-over-typeguard` | Narrowing predicates return TypeIs[T] (typing 3.13, typing_extensions), which also narrows the else branch; keep TypeGuard[T] only when T is not a subtype of the parameter's type. | python | ast-grep advise<br>advice, never blocks |
| `python/valid-pyproject` | pyproject.toml is valid: its [project] fields have the types PEP 621 gives them (dependencies is a list of strings, readme a string or a table). | any: `**/pyproject.toml` | tool ruff `RUF200` |
| `python/weak-crypto` | No insecure cipher (ARC4, Blowfish, IDEA, ...) or ECB mode, and no RSA/DSA key under 2048 bits or EC curve under 224 bits with the cryptography package. | python | tool ruff `S304`<br>tool ruff `S305`<br>tool ruff `S505` |
| `python/with-suffix-dot` | Path.with_suffix() gets a suffix with its dot (".txt"). | python | tool ruff `PTH210` |

### python/airflow

Apache Airflow DAGs.

Detected by: a dependency on `apache-airflow` (PyPI).
Child of `python`: active only where it is.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `python/airflow/airflow3` | No Airflow 2 API that Airflow 3 removed, moved or deprecated (ruff AIR301, AIR302, AIR303, AIR311, AIR312: days_ago, schedule_interval, execution_date, DummyOperator, airflow.operators.* now in the standard provider, airflow.models / airflow.decorators names now in airflow.sdk), and no direct metadata-database session (airflow.settings.Session, provide_session, create_session): Airflow 3 tasks reach the database only through the Task SDK and the API server. | python | tool ruff `AIR301`<br>tool ruff `AIR302`<br>tool ruff `AIR303`<br>tool ruff `AIR311`<br>tool ruff `AIR312`<br>tool ruff `TID251`<br>applies where `apache-airflow >=3.0.0` (PyPI) declared |
| `python/airflow/dag-schedule` | Every DAG() and @dag states schedule= (ruff AIR002), schedule=None included: the default changed between Airflow versions (None in Airflow 3), so an implicit schedule silently changes on upgrade. | python | tool ruff `AIR002` |

### python/anthropic

Anthropic Python SDK 2.

Detected by: a dependency on `anthropic >=2.0.0` (PyPI).
Child of `python`: active only where it is.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `python/anthropic/no-budget-tokens` | No thinking={"type": "enabled", "budget_tokens": N} in a messages call whose model= literal is one that rejects it with a 400 (Opus 4.7 and later, Sonnet 5 and 5.5, Fable 5 and 5.1, Mythos 5 and 5.1, Haiku 5.5; Bedrock's anthropic. prefix included): use thinking={"type": "adaptive"} and output_config={"effort": ...}. | python | ast-grep forbid (where `$C`: types engine, §15) |
| `python/anthropic/no-version-header` | No hand-set anthropic-version header in default_headers or extra_headers of an Anthropic client or call: it pins the app to older API behaviour than the SDK was built for. The SDK sends the right version. | python | ast-grep advise (where `$CALL`: types engine, §15)<br>advice, never blocks |
| `python/anthropic/stream-long-requests` | A messages.create() or messages.parse() call with max_tokens above 21,333 and no stream=True streams instead (client.messages.stream(...), then get_final_message()): the SDK refuses such a non-streaming request before sending it (expected to exceed 10 minutes), and with a custom timeout the HTTP request may time out. | python | ast-grep advise (where `$C`: types engine, §15)<br>advice, never blocks |

### python/asyncpg

asyncpg connection pools.

Detected by: a dependency on `asyncpg` (PyPI).
Child of `python`: active only where it is.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `python/asyncpg/pool-acquire-ctx` | A connection is taken from an asyncpg pool with async with pool.acquire() as conn:, not conn = await pool.acquire(): a bare acquire leaks the connection unless every path releases it in finally, and the pool runs dry. | python | ast-grep advise (where `$P`: types engine, §15)<br>advice, never blocks |

### python/attrs

attrs classes.

Detected by: a dependency on `attrs` (PyPI).
Child of `python`: active only where it is.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `python/attrs/next-gen-api` | Use attrs' current API (@attrs.define, @attrs.frozen, attrs.field), not the classic attr.s, attr.attrs, attr.ib, attr.attrib and attr.dataclass. | python | tool ruff `TID251` |

### python/celery

Celery tasks.

Detected by: a dependency on `celery` (PyPI).
Child of `python`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- Celery. Tasks take ids and plain JSON values and re-fetch rows when they run: a model instance passed as an argument is serialized when queued and stale when the task runs (rejected for Django and SQLAlchemy models).
- Tasks may run more than once (retries, `acks_late`, a worker lost mid-task): make them idempotent (check state before acting, use unique keys or upserts) and queue them from `transaction.on_commit()` when they read rows written in the same transaction.
- Retries use `autoretry_for` with `max_retries` and `retry_backoff`, or `self.retry(exc=exc)`; `max_retries=None` on a task, its `retry_kwargs`, `self.retry()` or `apply_async()` is rejected. A task never waits on another task's result (`.get()` in a task is rejected): use chains and chord callbacks.
- Celery messages use the json serializer: `pickle` in `task_serializer`, `result_serializer`, `event_serializer`, `serializer` or `accept_content` is rejected.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `python/celery/bounded-retries` | Celery retries are bounded: no max_retries=None (retry forever) in a @shared_task / @<app>.task decorator, its retry_kwargs, a self.retry(), apply_async(), signature() or .set() call, or as a class attribute of a Task subclass. Use autoretry_for with max_retries and retry_backoff, or self.retry() with Celery's default of 3. Other libraries' max_retries (requests' HTTPAdapter, urllib3) are not concerned. | python | ast-grep forbid |
| `python/celery/enqueue-on-commit` | A task is not sent (.delay() / .apply_async()) inside a Django transaction (with transaction.atomic():, an @transaction.atomic function): the worker can run before the transaction commits and read missing or stale rows, or run for a transaction that rolls back. Send it with transaction.on_commit(lambda: task.delay(…)) or task.delay_on_commit(…). | python | ast-grep forbid (where `$T`, `$C`, `$F`: types engine, §15)<br>applies where `django >=4.2` (PyPI) declared |
| `python/celery/no-model-args` | A task's arguments (.delay(…), .s(…), .si(…), apply_async(args=…, kwargs=…)) are not Django model instances: they are serialized at send time and stale (or unserializable) when the worker runs. Pass the primary key and load the row in the task. | python | ast-grep forbid (where `$T`, `$A`: types engine, §15)<br>applies where `django >=4.2` (PyPI) declared |
| `python/celery/no-pickle` | Celery does not use the pickle serializer: no task_serializer, result_serializer, event_serializer or serializer set to "pickle" (CELERY_* settings included), and no "pickle" or "application/x-python-serialize" in accept_content. Anyone who can publish to the broker could run code in the workers; use json. | python | grep forbid `((task\|result\|event)_)?serializer["']?[[:space:]]*[=:][[:space:]]*["']pickle["']\|accept_content.*["'](pickle\|application/x-python-serialize)["']` (ignoring case) |
| `python/celery/no-result-get-in-task` | A task does not wait for another task's result (.get(), .wait() or .join() on the AsyncResult of .delay(), .apply_async(), AsyncResult() or a called group/chord/chain): it holds a worker while waiting and can deadlock the pool; Celery refuses it unless disable_sync_subtasks=False, which is rejected too. Chain the tasks or use a chord callback. | python | ast-grep forbid |
| `python/celery/no-sqlalchemy-model-args` | A task's arguments (.delay(…), .s(…), .si(…), apply_async(args=…, kwargs=…)) are not SQLAlchemy mapped instances: they are serialized at send time, detached from their session and stale when the worker runs. Pass the primary key and load the row in the task. | python | ast-grep forbid (where `$T`, `$A`: types engine, §15)<br>applies where `sqlalchemy >=2.0` (PyPI) declared |

### python/django

Django models, migrations and ORM queries.

Detected by: a dependency on `django` (PyPI); or a file matching `manage.py`, `*/manage.py`.
Child of `python`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- Django. A model change comes with its migration: `python manage.py makemigrations`, then review and commit the file; `makemigrations --check --dry-run` finds nothing left to generate. Committed migrations are not edited.
- SQL in `.raw()`, `cursor.execute()` and `RawSQL()` takes `%s` placeholders and a params list: SQL built with an f-string, `.format()`, `%` or `+` is rejected, and so are `RawSQL()` and `QuerySet.extra()` with SQL that is not a literal.
- List views, serializers and templates that touch related objects load them with `select_related()` (foreign keys, one-to-one) or `prefetch_related()` (many-to-many, reverse relations); a query per row (N+1) is the usual slowdown.
- Several writes that must succeed or fail together run inside `transaction.atomic()`; work that must see the committed rows (tasks, emails) goes in `transaction.on_commit()`.
- HTML built in Python goes through `format_html()` (`mark_safe()` on text that is not a constant is rejected). `@csrf_exempt` is the user's decision: ask before adding it.
- Tests that assert what a `transaction.on_commit()` callback does run it with `django_capture_on_commit_callbacks(execute=True)` (pytest-django) or `TestCase.captureOnCommitCallbacks(execute=True)`, not with `django_db(transaction=True)` / `TransactionTestCase` for that alone.
- In `async def` code the ORM is used through its async API (`aget()`, `acount()`, `asave()`, `async for`); sync calls and `transaction.atomic` in async code are rejected: put transactional work in a sync function called through `sync_to_async`.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `python/django/applied-migrations` | Django migrations already in the base (file role @django-migrations) are not modified or deleted; they may already be applied, so a change goes in a new migration (makemigrations). | any | check migration: `forge_applied_migrations @django-migrations` |
| `python/django/async-orm` | Inside an async def, the ORM is used through its async API: asave() / adelete() / arefresh_from_db() on a model, aget() / afirst() / acount() / aexists() / acreate() / aupdate() / … on a QuerySet or manager, async for over a QuerySet. The sync calls raise SynchronousOnlyOperation when they run; wrap sync code in sync_to_async. | python | ast-grep forbid (where `$R`: types engine, §15) |
| `python/django/csrf-exempt` | No @csrf_exempt and no csrf_exempt(view): turning off CSRF protection is the user's decision. Ask them (is the endpoint authenticated by something other than cookies, such as a signed webhook?). | python | ast-grep forbid |
| `python/django/flake8-django` | ruff's flake8-django checks: no null=True on string fields (DJ001), no locals() as template context (DJ003), ModelForm lists its fields instead of exclude or "__all__" (DJ006, DJ007: new model fields would become editable), @receiver is the outermost decorator (DJ013: otherwise the handler is not connected). | python | tool ruff `DJ001`<br>tool ruff `DJ003`<br>tool ruff `DJ006`<br>tool ruff `DJ007`<br>tool ruff `DJ013` |
| `python/django/mailers-setting` | From Django 6.1, the email backend is configured in MAILERS, not the EMAIL_BACKEND / EMAIL_HOST / EMAIL_PORT / EMAIL_HOST_USER / EMAIL_HOST_PASSWORD / EMAIL_USE_TLS / EMAIL_USE_SSL / EMAIL_TIMEOUT / EMAIL_FILE_PATH / EMAIL_SSL_CERTFILE / EMAIL_SSL_KEYFILE settings, which are deprecated. | python: `@django-settings` | ast-grep forbid<br>applies where `django >=6.1` (PyPI) declared |
| `python/django/makemigrations` | Model changes come with their migration: python manage.py makemigrations --check --dry-run finds nothing to generate (run makemigrations and commit the new file). No database is needed; when manage.py cannot run here (settings that need an environment variable, a missing module), the check is skipped and says why. | python | check migration: a shell script of 12 lines (when `**/models.py`, `**/models/**`, `@django-migrations` or `@django-settings` changes; timeout 120 s) |
| `python/django/migrate-guard` | manage.py or django-admin migrate --fake, migrate <app> zero, flush, reset_db and sqlflush piped to a shell reset data or the database's migration state; confirm them. | any | guard ask: command `{name: [manage.py, django-admin], sub: [migrate], flags_any: [--fake]}` or `{name: [manage.py, django-admin], sub: [migrate], args_any: [zero]}` or `{name: [manage.py, django-admin], sub: [flush\|reset_db]}` or `{name: [manage.py, django-admin], sub: [sqlflush], pipe_to: ['*']}` |
| `python/django/migration-imports-models` | Migrations do not import the project's models modules (they would run against today's models, not the schema at that migration); RunPython code gets models with apps.get_model("app", "Model"). | python: `@django-migrations` | ast-grep forbid |
| `python/django/migration-reversible` | migrations.RunPython() has a reverse (reverse_code=, or migrations.RunPython.noop when there is nothing to undo) and migrations.RunSQL() a reverse_sql=, so the migration can be rolled back. | python: `@django-migrations` | ast-grep forbid |
| `python/django/no-atomic-in-async` | No transaction.atomic in or on an async def (with transaction.atomic(), @transaction.atomic): Django has no async transactions, so the block does not do what it says. Put the transactional work in a sync function and call it through sync_to_async. | python | ast-grep forbid (where `$C`, `$F`: types engine, §15) |
| `python/django/no-deprecated-api` | From Django 6.1, select_related() names the relations it follows, and values_list(flat=True) names its one field: both forms without arguments are deprecated (and select_related() with none follows every non-null foreign key). | python | ast-grep forbid (where `$Q`: types engine, §15)<br>applies where `django >=6.1` (PyPI) declared |
| `python/django/no-index-together` | A model's Meta lists its indexes in indexes = [models.Index(fields=[…])], not index_together: Django 5.1 removed index_together and ignores it in silence, so the index is never created. | python | ast-grep forbid (where `$B`: types engine, §15) |
| `python/django/no-mark-safe` | No mark_safe() or @mark_safe on text that is not a constant (ruff S308): a value marked safe is not escaped, so user input in it is cross-site scripting. Build HTML with format_html(), which escapes its arguments. | python | tool ruff `S308` |
| `python/django/no-raw-sql-format` | QuerySet.extra() and RawSQL() take no SQL built at run time (ruff S610, S611): pass constant SQL with %s placeholders and params, or use annotate() and filter() with expressions. SQL strings built with f-strings, .format(), % or + anywhere, a variable passed to .raw() or cursor.execute() included, are python/no-sql-format (ruff S608). | python: `!@django-migrations` | tool ruff `S610`<br>tool ruff `S611` |
| `python/django/no-template-unescape` | No \|safe or \|safeseq filter and no {% autoescape off %} in a Django template: the value is printed unescaped, so user input in it is cross-site scripting. Build the HTML in Python with format_html(), or keep autoescaping. | any: `@django-templates` | grep advise `\\|[[:space:]]*safe(seq)?([^A-Za-z_]\|$)\|\{%-?[[:space:]]*autoescape[[:space:]]+off`<br>advice, never blocks |
| `python/django/queryset-len` | len(queryset) loads every row to count them; when the rows are not used afterwards, queryset.count() asks the database for the number (a SELECT COUNT(*)). | python | ast-grep advise (where `$Q`: types engine, §15)<br>advice, never blocks |
| `python/django/storages-setting` | File and static storage backends are set in STORAGES ({"default": {…}, "staticfiles": {…}}), not DEFAULT_FILE_STORAGE or STATICFILES_STORAGE: Django 5.1 removed those settings and ignores them in silence, so uploads and collectstatic fall back to the local file system. | python: `@django-settings` | ast-grep forbid<br>applies where `django >=4.2` (PyPI) declared |
| `python/django/user-model-ref` | Relations to users name settings.AUTH_USER_MODEL, not django.contrib.auth.models.User nor the string "auth.User" (a hard-coded User breaks once the project has a custom user model). | python | ast-grep forbid (where `$F`, `$U`: types engine, §15) |

### python/django/drf

Django REST framework serializers and permissions.

Detected by: a dependency on `djangorestframework` (PyPI).
Child of `python/django`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- Django REST framework. A `ModelSerializer`'s `Meta.fields` lists the fields; `fields = "__all__"` is rejected. Fields the client must not set are in `read_only_fields`.
- Views inheriting a DRF view or viewset get a non-empty `permission_classes` (from the class or a project base or mixin) unless the settings Django runs with set a non-`AllowAny` `DEFAULT_PERMISSION_CLASSES` (DRF's default is `AllowAny`); `permission_classes = []` is rejected there; `AllowAny` on a write endpoint, or as the default, is rejected: ask the user first.
- Querysets behind list endpoints and nested serializers use `select_related()` / `prefetch_related()`.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `python/django/drf/no-allowany-write` | AllowAny is not set on an endpoint that writes (a ModelViewSet, a Create/Update/Destroy view or mixin, a view with post/put/patch/delete, an @api_view accepting POST/PUT/PATCH/DELETE) nor made the DEFAULT_PERMISSION_CLASSES: opening a write endpoint to anonymous callers is the user's decision. Ask them first. | python: `!**/tests/**`, `!**/test_*.py` | ast-grep forbid |
| `python/django/drf/no-csrf-exempt-session-auth` | No SessionAuthentication subclass that overrides enforce_csrf (the "CsrfExemptSessionAuthentication" snippet): it turns off CSRF protection for cookie-authenticated API calls. Send the CSRF token from the client, or authenticate the client with tokens instead of the session. | python | ast-grep forbid (where `$B`: types engine, §15) |
| `python/django/drf/permission-classes` | Unless the settings Django runs with (DJANGO_SETTINGS_MODULE from manage.py, wsgi.py or asgi.py, star imports followed) set REST_FRAMEWORK DEFAULT_PERMISSION_CLASSES to a non-empty list without AllowAny (DRF's default is AllowAny), a class inheriting a DRF APIView, generic view or viewset, directly or through the project's own classes and mixins, gets a non-empty permission_classes (or get_permissions) from itself or a project ancestor, and an @api_view function has @permission_classes. permission_classes = [] declares nothing. | python: `!@django-migrations`, `!**/tests/**`, `!**/test_*.py` | check lint: `forge_helper python drf_permissions.py` |
| `python/django/drf/serializer-fields` | A ModelSerializer's Meta lists its fields: fields = "__all__" and exclude = [...] would expose (and, unless read-only, accept) every column added to the model later, such as a password hash or an is_staff flag. | python: `!**/tests/**`, `!**/test_*.py` | ast-grep forbid |

### python/django/ninja

Django Ninja schemas.

Detected by: a dependency on `django-ninja` (PyPI).
Child of `python/django`: active only where it is.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `python/django/ninja/schema-fields` | A ModelSchema's Meta lists its fields: fields = "__all__" and exclude = [...] would expose every column added to the model later, such as a password hash or an is_staff flag. | python | ast-grep forbid (where `$B`: types engine, §15) |

### python/fastapi

FastAPI conventions.

Detected by: a dependency on `fastapi` (PyPI).
Child of `python`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- FastAPI. Dependencies use `Annotated[T, Depends(...)]`. Startup and shutdown use `lifespan` (`@app.on_event`, `on_startup=` and `on_shutdown=` are rejected).
- (when `fastapi >=0.100.0` (PyPI) declared) FastAPI reads request and response bodies from Pydantic v2 models.
- `FastAPI(debug=True)` is rejected outside tests. Tests enter `with TestClient(app) as client:` so the lifespan runs; async tests use `httpx.AsyncClient(transport=httpx.ASGITransport(app=app), base_url="http://test")` (TestClient in an `async def` is rejected).
- HTTP Basic credentials (`HTTPBasicCredentials.username` / `.password`) are compared with `secrets.compare_digest`, never `==`.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `python/fastapi/annotated-depends` | FastAPI dependencies use Annotated[T, Depends(dep)], not a Depends() default value. | python | tool ruff `FAST002` |
| `python/fastapi/basic-auth-compare-digest` | HTTP Basic credentials (HTTPBasicCredentials.username / .password) are compared with secrets.compare_digest, never == or !=, which leak through timing how much of the value matched. | python | ast-grep forbid (where `$C`: types engine, §15) |
| `python/fastapi/cors-credentials-wildcard` | CORS never combines allow_origins=["*"] with allow_credentials=True. List the allowed origins. | python | ast-grep forbid |
| `python/fastapi/deprecated-params` | No FastAPI parameters deprecated only at run time. Query/Path/Header/Cookie/Body/Form/File take pattern= (not regex=) and examples= or openapi_examples= (not example=); FastAPI() takes no root_path= (pass --root-path to fastapi run or uvicorn). | python | ast-grep forbid (where `$F`, `$APP`: types engine, §15) |
| `python/fastapi/no-debug-true` | No FastAPI(debug=True) or Starlette(debug=True) outside tests: debug mode returns tracebacks (code, settings, local values) to any client that triggers an error. | python: `!**/tests/**`, `!**/test_*.py`, `!**/*_test.py`, `!**/conftest.py` | ast-grep forbid (where `$C`: types engine, §15) |
| `python/fastapi/no-on-event` | FastAPI() and APIRouter() take no on_startup or on_shutdown handlers: startup and shutdown go in the lifespan context manager (FastAPI(lifespan=lifespan)). The @app.on_event decorator is deprecated as well. | python | ast-grep forbid (where `$C`: types engine, §15) |
| `python/fastapi/no-raw-request-body` | A route reads its input from a declared parameter (a Pydantic model body, Form(), File()), not from request.json(), .form(), .body() or .stream() on the Starlette Request, which skip validation and the OpenAPI schema. Reading the raw body stays right where the bytes matter, such as a webhook signature check. | python | ast-grep advise (where `$REQ`: types engine, §15)<br>advice, never blocks |
| `python/fastapi/no-testclient-in-async-test` | No TestClient inside an async def: it runs the app on another event loop in a thread, so async fixtures (database sessions, pools) cross loops and fail or hang. Use httpx.AsyncClient(transport=httpx.ASGITransport(app=app), base_url="http://test"). | python: `@python-tests` | ast-grep forbid (where `$C`: types engine, §15) |
| `python/fastapi/testclient-lifespan` | TestClient(app) is entered as a context manager (with TestClient(app) as client:, a fixture yielding inside the with): otherwise the app's lifespan never runs, and startup work such as opening the database pool is skipped. | python: `@python-tests` | ast-grep advise (where `$C`: types engine, §15)<br>advice, never blocks |
| `python/fastapi/unused-path-param` | Every {param} in a route's path is a parameter of its function (ruff FAST003): a path parameter missing from the signature is ignored, and the route serves the wrong resource. | python | tool ruff `FAST003` |

### python/flask

Flask applications.

Detected by: a dependency on `flask` (PyPI).
Child of `python`: active only where it is.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `python/flask/no-debug-run` | No app.run(debug=True) on a Flask app (ruff S201): the Werkzeug debugger lets anyone who reaches the server run code. Use flask run --debug in development. | python | tool ruff `S201` |
| `python/flask/no-template-string-injection` | render_template_string() gets a constant template; values go in as context keyword arguments, never into the template through an f-string, +, % or .format() (server-side template injection runs code through Jinja). | python | ast-grep forbid (where `$F`: types engine, §15) |

### python/flask/sqlalchemy

Flask-SQLAlchemy 3 with SQLAlchemy 2.0 queries.

Detected by: a dependency on `flask-sqlalchemy` (PyPI).
Child of `python/flask`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- Flask-SQLAlchemy 3 with SQLAlchemy 2.0 queries: `db.session.execute(db.select(User).filter_by(...))`, `db.session.scalars(...)`, `db.session.get(User, id)`, `db.get_or_404(User, id)`, `db.paginate(db.select(User))`. The legacy `User.query` is rejected.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `python/flask/sqlalchemy/no-model-query` | No legacy Model.query on Flask-SQLAlchemy models: use db.session.execute(db.select(User)...), db.session.scalars(...), db.session.get(User, id), db.get_or_404(User, id) or db.paginate(db.select(User)). | python | ast-grep forbid (where `$M`: types engine, §15) |

### python/huggingface

Hugging Face model and dataset loading.

Detected by: a dependency on one of `transformers` (PyPI), `datasets` (PyPI), `huggingface-hub` (PyPI), `sentence-transformers` (PyPI), `diffusers` (PyPI), `peft` (PyPI), `vllm` (PyPI).
Child of `python`: active only where it is.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `python/huggingface/no-pickle-weights` | No use_safetensors=False: it loads the pickled .bin weights even when safetensors exist, and unpickling runs any code the file holds. Leave the default, which prefers safetensors. | python | ast-grep forbid |
| `python/huggingface/no-trust-remote-code` | trust_remote_code=True (from_pretrained, pipeline, load_dataset, SentenceTransformer, vLLM) runs Python from the model or dataset repository. Prefer a model that does not need it; otherwise pin the revision= that was reviewed. | python | ast-grep advise<br>advice, never blocks |

### python/jupyter

Jupyter notebooks in the repository.

Detected by: a file matching `**/*.ipynb`.
Child of `python`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- Notebooks are committed with their outputs cleared and execution counts null.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `python/jupyter/no-outputs` | Notebooks are committed without outputs or execution counts (nbformat's output_type and a non-null execution_count): outputs carry data, credentials and large blobs into the history. Clear them (jupyter nbconvert --clear-output --inplace, or nbstripout). | any: `**/*.ipynb` | grep forbid `"(output_type\|execution_count)"[[:space:]]*:[[:space:]]*("\|[0-9])` |

### python/langchain

LangChain loaders, tools and serialization.

Detected by: a dependency on `langchain-core` (PyPI).
Child of `python`: active only where it is.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `python/langchain/no-dangerous-flags` | allow_dangerous_deserialization=True, allow_dangerous_code=True and allow_dangerous_requests=True opt in to unpickling, running model-written code and requests to any URL. Keep them for data and tools the project controls (an index it wrote itself), and say so. | python | ast-grep advise<br>advice, never blocks |
| `python/langchain/no-legacy-chains` | No langchain_classic imports (LLMChain, ConversationChain, RetrievalQA, initialize_agent, AgentExecutor, ...): the legacy chains and agents left langchain 1.0. Compose LCEL runnables, or use langchain.agents.create_agent or LangGraph. | python | tool ruff `TID251` |
| `python/langchain/no-python-repl` | No PythonREPLTool, PythonAstREPLTool or PythonREPL (langchain_experimental): they run model-written code in the app's process, with its permissions and secrets. | python | tool ruff `TID251` |
| `python/langchain/partner-packages` | No langchain_community class that LangChain deprecated in favour of a partner package (ChatOpenAI, OpenAIEmbeddings, HuggingFaceEmbeddings, Chroma, Ollama, ...): import it from that package (langchain_openai, langchain_huggingface, langchain_chroma, langchain_ollama, ...). The list is langchain-community 0.4.2's @deprecated(alternative_import=...) classes, at their defining module and their package re-export. | python | tool ruff `TID251` |
| `python/langchain/safe-load` | langchain_core.load's loads() and load() never take allowed_objects="all" or secrets_from_env=True: deserializing a manifest could then build any class and read the process's secrets (CVE-2025-68664). List the allowed objects; pass secrets_map= for the secrets it needs. | python | ast-grep forbid |

### python/library

A Python library's public API (__all__, deprecations).

Detected by: the project publishes a library to PyPI.
Child of `python`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- This project is a library: its public API is what `__all__` lists in its public modules (and what `__init__.py` re-exports).
- A new public function or class in a module with `__all__` is added to `__all__` (unlisted public names increasing is rejected), or named with a leading underscore.
- A public name is not removed or renamed outright: it stays importable and warns (`warnings.warn(..., DeprecationWarning, stacklevel=2)`, `typing_extensions.deprecated`); removing it, even in a major version, is the user's decision: ask them. A name leaving `__all__` without such a warning call or decorator (a docstring saying "deprecated" is not one) is rejected.
- `pyproject.toml`: `license` is an SPDX string with `license-files` (PEP 639; the build backend must support it: setuptools >= 77, hatchling >= 1.27, flit-core >= 3.12, pdm-backend >= 2.4, uv_build) and there are no `License ::` classifiers; `requires-python` has no upper bound.
- Library code never configures logging (`logging.basicConfig()`, handlers on the root logger); it logs through `logging.getLogger(__name__)`.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `python/library/all-exports` | In a module that defines __all__, a new public top-level function or class (a name without a leading underscore) is listed in __all__, or named with a leading underscore if it is internal. Counted per module: more unlisted public names than in the base fails. | python: `!**/tests/**`, `!@python-tests`, `!**/__main__.py` | ast-grep no-increase |
| `python/library/deprecation-shim` | A name that a module's __all__ listed in the base and no longer lists (removed or renamed), or a deleted module that had __all__, keeps working with a deprecation, read from the syntax tree: a def or class of that name decorated with @deprecated (typing_extensions, warnings) or calling warnings.warn with DeprecationWarning, FutureWarning, PendingDeprecationWarning or a subclass (directly or through a project function that does), an alias wrapped in deprecated(...)(...), or a module __getattr__ that names it and warns that way. The word "deprecated" in a docstring or comment does not count. A major version bump does not allow a removal by itself: removing a name is the user's decision: ask them. | python: `!**/tests/**`, `!**/test_*.py`, `!**/conftest.py` | check lint: `forge_helper python public_api.py` (when `**/*.py` or `pyproject.toml` changes) |
| `python/library/no-logging-config` | Library code does not configure logging (no logging.basicConfig(), no handler on the root logger): the application that imports it decides; a library logs through logging.getLogger(__name__). | python: `!**/tests/**`, `!**/test_*.py`, `!**/*_test.py`, `!**/conftest.py`, `!**/__main__.py` | ast-grep forbid |
| `python/library/no-python-cap` | requires-python has a lower bound only (>=3.12): an upper bound (<4, ~=, ==) makes resolvers pick old releases or fail for users on a newer Python. | any: `**/pyproject.toml` | grep forbid `^[[:space:]]*requires-python[[:space:]]*=.*(<\|~=\|==)` |
| `python/library/py-typed` | The library's wheel ships py.typed in each of its packages (PEP 561): without it, type checkers ignore the library's annotations and its users see Any. Checked on a wheel built with uv build when pyproject.toml or a py.typed file changes. | any: `**/pyproject.toml`, `**/py.typed` | check lint: a shell script calling `forge_do uv build-wheel`, `forge_do uv run-python` (when `pyproject.toml` or `**/py.typed` changes) |
| `python/library/spdx-license` | [project] license is an SPDX expression string (PEP 639: license = "MIT", with license-files), not a {text = ...} / {file = ...} table, and there is no License :: classifier. | any: `**/pyproject.toml` | grep forbid `^[[:space:]]*license[[:space:]]*=[[:space:]]*\{\|"License ::` |

### python/litestar

Litestar route handlers and application settings.

Detected by: a dependency on `litestar` (PyPI).
Child of `python`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- Litestar. A sync `def` route handler states `sync_to_thread=True` when it blocks (database drivers, file I/O) and `sync_to_thread=False` when it does not; async handlers use async clients. `Litestar(debug=True)` is rejected outside tests.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `python/litestar/no-debug-true` | No Litestar(debug=True) outside tests: debug mode returns tracebacks (code, settings, local values) to any client that triggers an error. | python: `!**/tests/**`, `!**/test_*.py`, `!**/*_test.py`, `!**/conftest.py` | ast-grep forbid (where `$C`: types engine, §15) |
| `python/litestar/sync-to-thread` | A sync def route handler (@get, @post, @put, @patch, @delete, @route, …) states sync_to_thread=True (it blocks: run it in a thread) or sync_to_thread=False (it does not): otherwise Litestar warns at startup, and a blocking handler run on the event loop stalls every request. | python | ast-grep forbid (where `$D`: types engine, §15) |

### python/llamaindex

LlamaIndex applications.

Detected by: a dependency on `llama-index-core` (PyPI).
Child of `python`: active only where it is.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `python/llamaindex/no-experimental` | No llama_index.experimental (PandasQueryEngine, PolarsQueryEngine, JSONalyzeQueryEngine): the package is deprecated and unmaintained, and its pandas and polars engines eval model-written code. | python | tool ruff `TID251` |
| `python/llamaindex/no-service-context` | No ServiceContext or set_global_service_context: LlamaIndex removed them (constructing one raises). Configure llama_index.core.Settings, or pass llm= / embed_model= to the component. | python | tool ruff `TID251` |

### python/loguru

loguru logging.

Detected by: a dependency on `loguru` (PyPI).
Child of `python`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- loguru. A message is either an f-string or a `{}` format string with arguments (`logger.info("order {} paid", order_id)`), never an f-string plus arguments.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `python/loguru/no-fstring-with-args` | With loguru, a message is either an f-string or a format string with arguments, never both: logger.info(f"{x} {}", y) formats the f-string's braces a second time (KeyError, IndexError or a garbled message). | python | ast-grep forbid (where `$L`: types engine, §15) |

### python/mcp

MCP Python SDK servers.

Detected by: a dependency on `mcp` (PyPI).
Child of `python`: active only where it is.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `python/mcp/dns-rebinding-protection` | An MCP HTTP server keeps DNS rebinding protection: no TransportSecuritySettings(enable_dns_rebinding_protection=False), which lets any web page the user opens reach a local server. List allowed_hosts and allowed_origins instead. | python | ast-grep forbid (where `$CALL`: types engine, §15) |

### python/msgspec

msgspec decoding.

Detected by: a dependency on `msgspec` (PyPI).
Child of `python`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- msgspec. Decoding validates against a type: `msgspec.json.decode(raw, type=Model)` (or a typed `msgspec.json.Decoder(Model)`), never an untyped `decode(raw)`.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `python/msgspec/typed-decode` | msgspec.json / msgpack / yaml / toml .decode() passes type= (a Struct or another type) so the input is validated; without it the result is untyped Any. | python | ast-grep forbid |

### python/numpy

NumPy random generators, arrays and files.

Detected by: a dependency on `numpy` (PyPI).
Child of `python`: active only where it is.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `python/numpy/legacy-random` | Random numbers come from a Generator (rng = np.random.default_rng(seed); rng.random()), not the legacy global RandomState functions (np.random.seed, np.random.rand, ...; ruff NPY002): global state is shared by every library and thread, so results are not reproducible. | python | tool ruff `NPY002` |
| `python/numpy/no-allow-pickle` | No numpy.load(..., allow_pickle=True). Loading a pickled object array runs code from the file; store plain arrays. | python | ast-grep forbid (where `$F`: types engine, §15) |

### python/opentelemetry

OpenTelemetry SDK setup.

Detected by: a dependency on `opentelemetry-sdk` (PyPI).
Child of `python`: active only where it is.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `python/opentelemetry/batch-processor` | Spans and log records go to an OTLP exporter through BatchSpanProcessor / BatchLogRecordProcessor, not SimpleSpanProcessor / SimpleLogRecordProcessor, which export synchronously on every span and block the request. The console exporter is not concerned. | python | ast-grep advise (where `$CALL`, `$E`: types engine, §15)<br>advice, never blocks |

### python/pandas

pandas 3 (Copy-on-Write, the str dtype, the new frequency aliases).

Detected by: a dependency on `pandas >=3.0.0` (PyPI).
Child of `python`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- pandas 3: Copy-on-Write. Assign through one indexer (`df.loc[row, col] = v`), never a chain (`df["a"][0] = v`); assign results (`df = df.rename(...)`) instead of `inplace=True` on a selection or on the methods that deprecate it.
- Strings have the `str` dtype: test with `pd.api.types.is_string_dtype()` or `select_dtypes("string")`, not `dtype == object`. Offsets are `ME`, `QE`, `YE`, `h`, `min`, `s`, `ms`, `us`, `ns` (`M`, `H`, `T`, `S`, `A`, ... raise). Prefer column operations to row loops.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `python/pandas/no-chained-assignment` | No chained assignment on a pandas object (df["a"][0] = v, df.loc[0]["a"] = v, df["a"].iloc[0] = v, and += / -=): under Copy-on-Write (pandas 3) the first selection is a copy, so nothing is written. Use one indexer: df.loc[row, col] = v. | python | ast-grep forbid (where `$D`: types engine, §15) |
| `python/pandas/no-chained-inplace` | No inplace=True on a selection (df["a"].fillna(0, inplace=True), df.loc[...].replace(..., inplace=True)): under Copy-on-Write (pandas 3) the selection is a copy, so df never changes. Assign the result: df["a"] = df["a"].fillna(0). | python | ast-grep forbid (where `$D`: types engine, §15) |
| `python/pandas/no-deprecated-inplace` | No inplace=True on the DataFrame and Series methods pandas 3.1 deprecates it for (rename, rename_axis, reset_index, set_index, drop, drop_duplicates, dropna, sort_values, sort_index, query, eval): a Pandas4Warning now, removed in pandas 4. Assign the result: df = df.rename(...). fillna, replace, where, mask, clip, interpolate, ffill and bfill keep inplace. | python | ast-grep forbid (where `$R`: types engine, §15) |
| `python/pandas/no-object-dtype-check` | Strings are not found with dtype == object (or "O", np.object_) nor select_dtypes("object"): pandas 3 stores strings in the str dtype, so the comparison is False and select_dtypes includes them only for backward compatibility. Use pd.api.types.is_string_dtype() or select_dtypes("string"). | python | ast-grep forbid (where `$R`, `$D`: types engine, §15) |
| `python/pandas/no-removed-freq-aliases` | No frequency aliases pandas 3 removed (they raise ValueError): BM, SM, CBM, BQ, BY, A, AS, BA, BAS, H, BH, CBH, T, L, U, N, S (with a number or a -JAN anchor), and M, Q, Y as offsets (resample, date_range, Grouper, rolling); M, Q and Y stay valid periods (to_period, period_range, Period, PeriodIndex). Use ME, QE, YE, h, min, s, ms, us, ns. | python | ast-grep forbid (where `$CALL`: types engine, §15) |
| `python/pandas/no-row-loops` | No row loops over a DataFrame (iterrows(), itertuples(), apply(..., axis=1)): they run Python per row, orders of magnitude slower than column operations. Write vectorized expressions (df["a"] * df["b"], np.where, merge). | python | ast-grep advise (where `$R`: types engine, §15)<br>advice, never blocks |

### python/pandera

pandera dataframe schemas.

Detected by: a dependency on `pandera` (PyPI).
Child of `python`: active only where it is.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `python/pandera/pandas-namespace` | pandas schemas come from pandera.pandas (import pandera.pandas as pa), not the top-level pandera namespace, which pandera deprecated for them: DataFrameSchema, SeriesSchema, Column, Index, MultiIndex, DataFrameModel, infer_schema, check_types, check_input, check_output, check_io. Check, Field, check, dataframe_check and parser stay top-level. | python | tool ruff `TID251` |

### python/pdm

PDM is the Python package manager.

Detected by: the fact `python.pm` is `pdm`.
Child of `python`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- The project is managed with PDM: dependencies are added with `pdm add <pkg>` (`pdm add --dev <pkg>` for tooling), and tools run through the project's environment (`pdm run <tool>`).

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `python/pdm/no-other-pm` | Dependencies are added with pdm add (the lock file is pdm.lock); Claude asks before pip installs, other package managers or a hand-made virtualenv. | any | guard ask: command `{name: [pip, pip3, pip3.*], sub: [install], flags_none: [--dry-run]}` or `{name: uv, sub: [pip, install], flags_none: [--dry-run]}` or `{name: [conda, mamba, micromamba], sub: [install]}` or `{name: [venv, virtualenv]}` or `{name: [uv, poetry], sub: [add]}` or `{name: pipenv, sub: [install]}` |

### python/piccolo

Piccolo ORM queries.

Detected by: a dependency on `piccolo` (PyPI).
Child of `python`: active only where it is.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `python/piccolo/no-run-sync-in-async` | No .run_sync() on a Piccolo query inside an async def: with a running event loop it runs the query in a thread pool and blocks the loop until it returns. Await the query (await Band.select()). | python | ast-grep forbid (where `$Q`: types engine, §15) |

### python/pip

pip with requirements files.

Detected by: the fact `python.pm` is `pip`.
Child of `python`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- (when a file matches `requirements*.txt`, `**/requirements*.txt`, `**/requirements/*.txt`) Dependencies are listed in the requirements files with exact versions (`name==1.2.3`): a new dependency goes there with the version to use, then `pip install -r <file>` installs it.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `python/pip/pinned-requirements` | Requirements files pin each package to an exact version (name==1.2.3), so every install gets the same packages; a range or a bare name lets each machine resolve its own. | any: `**/requirements*.txt`, `**/requirements/*.txt` | grep forbid `^[[:space:]]*[A-Za-z0-9][A-Za-z0-9._-]*(\[[^]]*\])?[[:space:]]*($\|[;#<>!~])` |

### python/pipenv

Pipenv is the Python package manager.

Detected by: the fact `python.pm` is `pipenv`.
Child of `python`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- The project is managed with Pipenv: dependencies are added with `pipenv install <pkg>` (`pipenv install --dev <pkg>` for tooling), and tools run through the project's environment (`pipenv run <tool>`).

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `python/pipenv/no-other-pm` | Dependencies are added with pipenv install (the lock file is Pipfile.lock); Claude asks before pip installs, other package managers or a hand-made virtualenv. | any | guard ask: command `{name: [pip, pip3, pip3.*], sub: [install], flags_none: [--dry-run]}` or `{name: uv, sub: [pip, install], flags_none: [--dry-run]}` or `{name: [conda, mamba, micromamba], sub: [install]}` or `{name: [venv, virtualenv]}` or `{name: [uv, poetry, pdm], sub: [add]}` |

### python/poetry

Poetry is the Python package manager.

Detected by: the fact `python.pm` is `poetry`.
Child of `python`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- The project is managed with Poetry: dependencies are added with `poetry add <pkg>` (`poetry add --group dev <pkg>` for tooling), and tools run through the project's environment (`poetry run <tool>`).

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `python/poetry/no-other-pm` | Dependencies are added with poetry add (the lock file is poetry.lock); Claude asks before pip installs, other package managers or a hand-made virtualenv. | any | guard ask: command `{name: [pip, pip3, pip3.*], sub: [install], flags_none: [--dry-run]}` or `{name: uv, sub: [pip, install], flags_none: [--dry-run]}` or `{name: [conda, mamba, micromamba], sub: [install]}` or `{name: [venv, virtualenv]}` or `{name: [uv, pdm], sub: [add]}` or `{name: pipenv, sub: [install]}` |

### python/polars

Polars expressions.

Detected by: a dependency on `polars` (PyPI).
Child of `python`: active only where it is.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `python/polars/no-python-row-functions` | No Python per row in Polars (Expr.map_elements, Series.map_elements, DataFrame.map_rows, DataFrame.iter_rows): it runs one Python call per value, without parallelism or the query optimizer. Write Polars expressions (pl.col("a") + 1, pl.when()); map_batches is not concerned. | python | ast-grep advise (where `$E`: types engine, §15)<br>advice, never blocks |

### python/prefect

Prefect flows and deployments.

Detected by: a dependency on `prefect` (PyPI).
Child of `python`: active only where it is.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `python/prefect/no-removed-v2-api` | No Prefect 2 API that Prefect 3 removed or moved (from Prefect's own migration table): Deployment.build_from_flow (use flow.deploy() or flow.serve()), the S3 / GCS / Azure / GitHub filesystem blocks (use the prefect-aws, prefect-gcp, prefect-azure, prefect-github blocks), agents and infrastructure blocks (use workers), and the moved names (load_flow_from_flow_run, variables.get, pause_flow_run / resume_flow_run / suspend_flow_run, prefect.client.get_client). | python | tool ruff `TID251`<br>applies where `prefect >=3.0.0` (PyPI) declared |

### python/psycopg

psycopg 3 connections, cursors and pools.

Detected by: a dependency on one of `psycopg` (PyPI), `psycopg-pool` (PyPI).
Child of `python`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- psycopg 3. SQL takes `%s` or `%(name)s` placeholders with a params tuple or dict (not asyncpg's `$1`); dynamic identifiers go through `psycopg.sql` (`sql.SQL`, `sql.Identifier`). Pools are created with `open=False` and opened with `pool.open()` (awaited for `AsyncConnectionPool`) in the application's startup, or used as a context manager.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `python/psycopg/placeholder-style` | SQL passed to a psycopg execute() or executemany() takes %s or %(name)s placeholders, not $1 (asyncpg's style, which psycopg sends to the server unchanged and PostgreSQL rejects). Text with a dollar-quoted body ($$ ... $$) is left alone. | python | ast-grep advise (where `$R`: types engine, §15)<br>advice, never blocks |
| `python/psycopg/pool-open-explicit` | ConnectionPool() and AsyncConnectionPool() pass open= unless entered with with / async with: opening the pool in the constructor is deprecated, and an async pool cannot open there (RuntimeWarning). Use open=False, then await pool.open() in the application's startup (lifespan). | python | ast-grep forbid (where `$C`: types engine, §15) |

### python/pydantic

Pydantic v2 APIs only.

Detected by: a dependency on one of `pydantic >=2.0.0` (PyPI), `fastapi >=0.100.0` (PyPI).
Child of `python`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- Pydantic v2: `model_validate`, `model_dump`, `model_config = ConfigDict(...)`, `@field_validator`, `@model_validator`, `Field(pattern=...)`, `TypeAdapter(T).validate_python(...)` (not `parse_obj_as`), with the adapter built once at module level.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `python/pydantic/no-v1-namespace` | No pydantic.v1 imports: the v1 compatibility namespace keeps v1 behaviour; use the pydantic v2 API. | python | tool ruff `TID251` |
| `python/pydantic/typeadapter-module-level` | A TypeAdapter is built once, at module level or in a cached function (@cache, @lru_cache, @cached_property), not in a function body: each construction builds the validator and serializer again. | python | ast-grep advise (where `$C`: types engine, §15)<br>advice, never blocks |
| `python/pydantic/v2-api` | No Pydantic v1 configuration: no class Config in a BaseModel or BaseSettings subclass (use model_config = ConfigDict(...) / SettingsConfigDict(...)), no json_encoders (use @field_serializer or PlainSerializer), no v1-only Field() arguments (env, regex, const, min_items, max_items, unique_items, allow_mutation, example). | python | ast-grep forbid (where `$B`, `$C`, `$F`: types engine, §15) |

### python/pydantic/settings

pydantic-settings configuration classes.

Detected by: a dependency on `pydantic-settings` (PyPI).
Child of `python/pydantic`: active only where it is.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `python/pydantic/settings/no-load-dotenv` | Settings read .env through SettingsConfigDict(env_file=...), not dotenv.load_dotenv(): pydantic-settings reads the file itself, and load_dotenv() also copies it into os.environ for the whole process. | python | tool ruff `TID251` |

### python/pyjwt

PyJWT token verification.

Detected by: a dependency on `pyjwt` (PyPI).
Child of `python`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- PyJWT. `jwt.decode(token, key, algorithms=["RS256"])` always names the accepted algorithms (never `"none"`) and never turns verification off (`options={"verify_signature": False}`, `verify=False`); read an unverified header with `jwt.get_unverified_header()`.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `python/pyjwt/jwt-verify` | jwt.decode() names the accepted algorithms (algorithms=[...], never "none") and keeps the signature check: no options={"verify_signature": False}, no verify=False. | python | ast-grep forbid (where `$J`: types engine, §15) |

### python/pytest

pytest runs and test integrity.

Detected by: a dependency on `pytest` (PyPI).
Child of `python`: active only where it is.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `python/pytest/django-deprecations-error` | When django is a dependency, pytest's filterwarnings turns Django's RemovedInNextVersionWarning (what the next Django release removes) into errors with "error::django.utils.deprecation.RemovedInNextVersionWarning" (or a bare "error"): as a warning it lets the tests pass on code the next release breaks. | any: `**/pyproject.toml`, `**/pytest.ini`, `**/.pytest.ini`, `**/tox.ini`, `**/setup.cfg` | check lint: a shell script calling `forge_helper` (when `pyproject.toml`, `pytest.ini`, `.pytest.ini`, `tox.ini` or `setup.cfg` changes)<br>applies where `django *` (PyPI) declared |
| `python/pytest/drf-deprecations-error` | When djangorestframework is a dependency, pytest's filterwarnings turns DRF's RemovedInNextDRFVersionWarning (what the next DRF release removes) into errors with "error::rest_framework.deprecation.RemovedInNextDRFVersionWarning" (or a bare "error"): as a warning it lets the tests pass on code the next release breaks. | any: `**/pyproject.toml`, `**/pytest.ini`, `**/.pytest.ini`, `**/tox.ini`, `**/setup.cfg` | check lint: a shell script calling `forge_helper` (when `pyproject.toml`, `pytest.ini`, `.pytest.ini`, `tox.ini` or `setup.cfg` changes)<br>applies where `djangorestframework *` (PyPI) declared |
| `python/pytest/no-blind-raises` | pytest.raises names the exception expected, never bare Exception (which passes on any crash). | python: `@python-tests`, `!**/conftest.py` | tool ruff `B017` |
| `python/pytest/no-exit-zero` | Conftest and test files never exit with status 0 (sys.exit(0), os._exit(0), pytest.exit(..., returncode=0)): the run reports success whatever the tests did. | python: `**/test_*.py`, `**/*_test.py`, `**/conftest.py` | ast-grep forbid |
| `python/pytest/no-exitstatus` | An assignment to a pytest Session's exitstatus (decided by its type, pytest's Session) rewrites the suite's result; say what it is for. | python: `@python-tests` | ast-grep advise (where `$S`: types engine, §15)<br>advice, never blocks |
| `python/pytest/no-fewer-asserts` | Assertions are not removed from tests. | python: `@python-tests`, `!**/conftest.py` | ast-grep no-decrease |
| `python/pytest/no-hook-tamper` | A new pytest hook that can rewrite results (pytest_runtest_makereport, pytest_report_teststatus, pytest_runtest_call, pytest_pyfunc_call, pytest_runtest_protocol, pytest_terminal_summary, pytest_collection_modifyitems) in a conftest or test file; say what it is for. Rewriting reports or deselecting tests makes a failing suite pass. | python: `**/test_*.py`, `**/*_test.py`, `**/conftest.py` | ast-grep advise<br>advice, never blocks |
| `python/pytest/no-mock-increase` | A test file may not gain mocks: unittest.mock's patch (patch.object, patch.dict, patch.multiple), Mock, MagicMock, AsyncMock, NonCallableMock, create_autospec, under their names or an import alias (from unittest.mock import patch as p, import unittest.mock as um), and pytest-mock's mocker.patch*, mocker.spy, mocker.stub and mocker.Mock*; test through the real code or a fake at the boundary. monkeypatch (setattr, setenv, setitem) is not counted: it is pytest's way to set configuration for a test. | python: `@python-tests` | ast-grep no-increase |
| `python/pytest/no-new-skip` | Tests are not skipped, marked xfail or made to skip on a missing import to make the suite pass. | python: `@python-tests` | ast-grep no-increase |
| `python/pytest/no-tautological-assert` | Assertions test something. No assert True, assert on a literal, or x == x. | python: `@python-tests`, `!**/conftest.py` | ast-grep forbid |
| `python/pytest/no-test-edits` | Changing a test that existed at the start of the turn (anything in it, formatting aside, input data and control flow included), or removing it, needs approval; adding tests is fine. Edits ask when made, unapproved changes are blocked at Stop. Changed test support code (fakes, mocks, fixtures, helpers, conftest.py) is reported once. | python: `@python-tests` | check lint: `forge_test_edits` |
| `python/pytest/no-test-env-branch` | Application code does not branch on whether it runs under pytest. | python: `**/*.py`, `!@python-tests`, `!tests/**` | ast-grep forbid |
| `python/pytest/pydantic-deprecations-error` | When pydantic is a dependency, pytest's filterwarnings turns PydanticDeprecationWarning (the v1-style APIs Pydantic 3 removes) into errors with "error::pydantic.warnings.PydanticDeprecationWarning" (or a bare "error"): as a warning it lets the tests pass on code the next release breaks. | any: `**/pyproject.toml`, `**/pytest.ini`, `**/.pytest.ini`, `**/tox.ini`, `**/setup.cfg` | check lint: a shell script calling `forge_helper` (when `pyproject.toml`, `pytest.ini`, `.pytest.ini`, `tox.ini` or `setup.cfg` changes)<br>applies where `pydantic >=2.0.0` (PyPI) declared |
| `python/pytest/run` | Tests pass. | python | check test: `forge_do pytest test` (reruns failing tests; timeout 300 s) |
| `python/pytest/untested-change` | A turn that changes application code also adds or updates a test, or says why none is needed; advisory, it blocks Stop once per turn. | python: `**/*.py`, `!@python-tests`, `!**/tests/**`, `!@django-migrations`, `!@alembic-migrations` | check lint: `forge_advise forge_untested_change`<br>advice, never blocks |

### python/pytest/asyncio

pytest-asyncio fixtures and event loops.

Detected by: a dependency on `pytest-asyncio` (PyPI).
Child of `python/pytest`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- pytest-asyncio. Async tests are marked `@pytest.mark.asyncio` (or run in `asyncio_mode = "auto"`); async fixtures use `@pytest_asyncio.fixture`.
- The loop's lifetime is set with `loop_scope=` (`@pytest.mark.asyncio(loop_scope="session")`, `@pytest_asyncio.fixture(loop_scope=...)`) or `asyncio_default_fixture_loop_scope`; no `event_loop` fixture and no test requesting it (use `asyncio.get_running_loop()`); a custom loop comes from the `pytest_asyncio_loop_factories` hook, not an `event_loop_policy` fixture.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `python/pytest/asyncio/async-fixture-decorator` | async def fixtures use @pytest_asyncio.fixture, not @pytest.fixture (which strict mode, the default, does not run as a coroutine). | python: `**/test_*.py`, `**/*_test.py`, `**/conftest.py` | ast-grep forbid |
| `python/pytest/asyncio/no-event-loop-fixture` | No event_loop fixture (removed in pytest-asyncio 1.0) and no test requesting event_loop: choose the loop with @pytest.mark.asyncio(loop_scope=...), @pytest_asyncio.fixture(loop_scope=...) or asyncio_default_fixture_loop_scope, and get it with asyncio.get_running_loop(). | python: `**/test_*.py`, `**/*_test.py`, `**/conftest.py` | ast-grep forbid |
| `python/pytest/asyncio/no-loop-policy-fixture` | Do not override the event_loop_policy fixture (event loop policies are deprecated in Python 3.14): implement the pytest_asyncio_loop_factories hook instead. | python: `**/test_*.py`, `**/*_test.py`, `**/conftest.py` | ast-grep forbid |

### python/redis

redis-py clients.

Detected by: a dependency on one of `redis` (PyPI), `aioredis` (PyPI).
Child of `python`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- redis-py. Iterate keys with `scan_iter(match=...)`, never `KEYS`; asyncio clients (`redis.asyncio`) are closed with `await r.aclose()`.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `python/redis/aclose` | An asyncio Redis client is closed with await r.aclose(), not r.close(): close() is deprecated through redis' own decorator, which the type checker does not read. | python | ast-grep forbid (where `$R`: types engine, §15) |
| `python/redis/no-aioredis` | No aioredis: it is unmaintained (merged into redis-py 4.2); use redis.asyncio. | python | tool ruff `TID251` |
| `python/redis/no-keys` | Application code does not call KEYS (r.keys(pattern)): it walks every key and blocks Redis while it runs. Iterate with scan_iter(match=pattern). | python | ast-grep forbid (where `$R`: types engine, §15) |

### python/sentry

Sentry's Python SDK.

Detected by: a dependency on `sentry-sdk` (PyPI).
Child of `python`: active only where it is.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `python/sentry/no-deprecated-scope-api` | No sentry_sdk.configure_scope, push_scope, Hub (Hub.current) or set_measurement: Sentry SDK 2 deprecated them. Use sentry_sdk.get_isolation_scope() / get_current_scope(), new_scope() / isolation_scope(), set_tag(), and span attributes. | python | tool ruff `TID251`<br>applies where `sentry-sdk >=2.0.0` (PyPI) declared |
| `python/sentry/span-attributes` | sentry_sdk.update_current_span() takes attributes=, not the deprecated data= (deprecated in sentry-sdk 2.35, a runtime warning only). | python | ast-grep forbid (where `$F`: types engine, §15) |

### python/sqlalchemy

SQLAlchemy 2.0 typed APIs.

Detected by: a dependency on `sqlalchemy >=2.0.0` (PyPI).
Child of `python`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- SQLAlchemy 2.0. Models: `class Base(DeclarativeBase)`, `Mapped[T]`, `mapped_column()`, `relationship(back_populates=...)`. Queries: `select()` with `session.execute()` or `session.scalars()` (`session.query()` is rejected). Large collections are `WriteOnlyMapped[...]` (`lazy="dynamic"` is rejected).
- Values reach `text()` SQL as bound parameters (`:id`): an f-string, `.format()`, `%` or `+` inside `text()`, and a string literal passed to `.execute()`, are rejected. Statements that cannot take parameters (`SET`, `SET LOCAL`) may embed a validated value such as an int.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `python/sqlalchemy/async-expire-on-commit` | async_sessionmaker sets expire_on_commit=False, so attributes read after a commit do not trigger implicit IO (MissingGreenlet) in async code. | python | ast-grep forbid |
| `python/sqlalchemy/asyncio-extra` | When code imports sqlalchemy.ext.asyncio, the project declares sqlalchemy[asyncio] (or greenlet) as a dependency in pyproject.toml or a requirements*.txt: SQLAlchemy installs greenlet only through that extra (2.1), and without it the async engine fails at run time. | any | check lint: a shell script of 4 lines (when `pyproject.toml`, `**/pyproject.toml`, `requirements*.txt`, `**/requirements*.txt` or `**/*.py` changes) |
| `python/sqlalchemy/fk-index` | A foreign key column the turn adds to a model or Table() gets an index that leads with it (index=True, unique=True, the first primary key column, or an Index / UniqueConstraint / PrimaryKeyConstraint whose first column it is). PostgreSQL and SQLite do not index referencing columns, and PostgreSQL's documentation calls such an index often a good idea (joins, deletes of the referenced rows), not a requirement. Skipped when the database URL is MySQL or MariaDB, which index foreign keys themselves. | python | check lint: a shell script calling `forge_db_url`, `forge_advise`, `forge_helper`<br>advice, never blocks |
| `python/sqlalchemy/mapped-column` | Model attributes use Mapped[T] = mapped_column(), not Column(). | python | ast-grep forbid |
| `python/sqlalchemy/no-backref` | No relationship(backref=...). Declare both sides with back_populates. | python | ast-grep forbid |
| `python/sqlalchemy/no-concurrent-session` | No asyncio.gather() over calls on one AsyncSession: a session is not safe for concurrent use (InvalidRequestError, or corrupted state). Await the calls one after the other, or give each task its own session. | python | ast-grep forbid (where `$S`: types engine, §15) |
| `python/sqlalchemy/no-declarative-base` | No declarative_base(). Subclass DeclarativeBase. | python | ast-grep forbid |
| `python/sqlalchemy/no-lazy-dynamic` | No lazy="dynamic" relationships: dynamic loaders are legacy in SQLAlchemy 2.0 and do not work with asyncio. Declare the collection as WriteOnlyMapped[...] (lazy="write_only") and query it with select(). | python | ast-grep forbid (where `$C`: types engine, §15) |
| `python/sqlalchemy/no-query-api` | No legacy session.query(). Use select() with session.execute() or session.scalars(). | python | ast-grep forbid (where `$S`: types engine, §15) |
| `python/sqlalchemy/no-raw-execute` | No raw SQL strings in execute(). Use select() or text() with bound parameters. | python | ast-grep forbid |
| `python/sqlalchemy/no-text-fstring` | text() SQL is a constant with bound parameters (:id), never built with an f-string, .format() or %. | python | ast-grep forbid |
| `python/sqlalchemy/session-context` | A Session or AsyncSession (Session(engine), a sessionmaker's call) is opened in a with / async with block, or returned or yielded to the code that closes it (a framework dependency): a session left open keeps its connection. A try/finally that calls close() is also right. | python | ast-grep advise (where `$C`: types engine, §15)<br>advice, never blocks |

### python/sqlalchemy/alembic

Alembic migrations with an async engine.

Detected by: a dependency on `alembic` (PyPI); or a file matching `alembic.ini`.
Child of `python/sqlalchemy`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- Alembic. New revisions: `alembic revision --autogenerate -m "<change>"`. Committed revisions are never edited.
- (when pack `python/sqlalchemy/asyncio` active) With an async engine, `alembic/env.py` runs the migrations through `connection.run_sync()` (the async template's `run_async_migrations()`).
- Alembic owns the schema: tables are created by revisions, and `metadata.create_all()` outside tests is rejected.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `python/sqlalchemy/alembic/applied-migrations` | Alembic revisions already in the base (file role @alembic-migrations, the versions directory of alembic.ini's script_location) are not modified or deleted; they may already be applied, so a change goes in a new revision (alembic revision --autogenerate). | any | check migration: `forge_applied_migrations @alembic-migrations` |
| `python/sqlalchemy/alembic/check` | Model changes come with a migration (alembic check passes). | python | check migration: `forge_do alembic migrations-check` (when `**/models/**`, `**/models.py` or `@alembic-migrations` changes; needs a local database) |
| `python/sqlalchemy/alembic/data-migration-downgrade` | A revision whose upgrade() changes rows (UPDATE, INSERT or DELETE through execute(), sa.update/insert/delete, op.bulk_insert, an ORM Session) has a downgrade() that changes rows the same ways (the inverse) or raises: a downgrade that only passes, prints or changes the schema reports success and leaves the data changed. | python: `@alembic-migrations` | check lint: `forge_advise forge_alembic_data_downgrade`<br>advice, never blocks |
| `python/sqlalchemy/alembic/downgrade` | alembic downgrade and stamp change the database's revision; confirm them. | any | guard ask: command `{name: alembic, sub: [downgrade\|stamp]}` |
| `python/sqlalchemy/alembic/fk-index` | A revision that adds a foreign key also creates an index that leads with its column (the first column of op.create_index, sa.Index, a primary key or unique constraint, or CREATE INDEX; index=True, unique=True or primary_key=True on the column). PostgreSQL and SQLite do not index referencing columns, so joins and deletes of the referenced rows scan the table; PostgreSQL's documentation calls the index often a good idea, not a requirement. Skipped for a MySQL or MariaDB database URL (InnoDB indexes foreign keys). | python: `@alembic-migrations` | check lint: `forge_advise forge_alembic_fk_index`<br>advice, never blocks |
| `python/sqlalchemy/alembic/migration-lint` | New revisions do not break the running app during a deploy (no column or table renames, type changes, new required columns or DROP DATABASE); Squawk lints the SQL Alembic renders offline. | python: `@alembic-migrations` | check migration: `forge_squawk_alembic $(forge_do alembic heads \| forge_alembic_files)` (when `@alembic-migrations` changes) |
| `python/sqlalchemy/alembic/model-registered` | A new mapped class in a models package is imported where the other models are (the package's __init__.py, or alembic/env.py): otherwise Base.metadata lacks its table, autogenerate writes no migration for it and alembic check still passes. | python: `**/models/**` | check lint: `forge_models_registered` |
| `python/sqlalchemy/alembic/no-create-all` | No MetaData.create_all() outside tests (Base.metadata.create_all, SQLModel.metadata.create_all, conn.run_sync(Base.metadata.create_all)): Alembic manages the schema, and tables created behind its back diverge between environments and break the next autogenerate. Add a revision instead. | python: `!**/tests/**`, `!**/test_*.py`, `!**/*_test.py`, `!**/conftest.py` | ast-grep forbid (where `$M`: types engine, §15) |
| `python/sqlalchemy/alembic/no-downgrade-base` | alembic downgrade base drops the whole schema. | any | guard deny: command `{name: alembic, sub: [downgrade, base]}` |
| `python/sqlalchemy/alembic/single-head` | Alembic has a single head. | python | check migration: `test "$(forge_do alembic heads \| wc -l)" -le 1` (when `@alembic-migrations` changes) |
| `python/sqlalchemy/alembic/sqlite-batch` | When migrations also run on SQLite (render_as_batch=True in alembic/env.py, or a sqlite URL in alembic.ini), alter_column, drop_column and constraint operations go through op.batch_alter_table(): SQLite's ALTER TABLE cannot run them. | python: `@alembic-migrations` | check lint: `forge_alembic_batch` |

### python/sqlalchemy/asyncio

SQLAlchemy's asyncio extension.

Detected by: a dependency on one of `asyncpg` (PyPI), `aiosqlite` (PyPI), `aiomysql` (PyPI), `asyncmy` (PyPI); or a line matching `(?i)sqlalchemy\[[^\]]*asyncio` in a file matching `**/pyproject.toml`, `**/requirements*.txt`, `**/setup.cfg`, `**/setup.py`, `**/Pipfile`.
Child of `python/sqlalchemy`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- SQLAlchemy asyncio: sessions come from `async_sessionmaker(engine, expire_on_commit=False)` and are opened with `async with`.
- Async sessions cannot lazy-load (`MissingGreenlet`): load related rows with `selectinload()`, or declare `lazy="raise"`. One session never runs two statements at once (`asyncio.gather()` over one session is rejected).

No rules of its own.

### python/strawberry

Strawberry GraphQL schemas.

Detected by: a dependency on `strawberry-graphql` (PyPI).
Child of `python`: active only where it is.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `python/strawberry/query-limits` | A strawberry.Schema() limits query depth with extensions=[QueryDepthLimiter(max_depth=...)] (and consider MaxTokensLimiter, MaxAliasesLimiter): without limits, one deeply nested query costs the server far more than the client. | python | ast-grep advise (where `$C`: types engine, §15)<br>advice, never blocks |

### python/structlog

structlog structured logging.

Detected by: a dependency on `structlog` (PyPI).
Child of `python`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- structlog. Events are constant strings and the data goes in key/value pairs: `log.info("user_login", user_id=uid)`, never an f-string, `.format()` or `%` in the event.
- `structlog.get_logger()` is typed `Any`: annotate the logger (`log: structlog.typing.FilteringBoundLogger = structlog.get_logger()`, or `structlog.stdlib.get_logger()` with the stdlib wrapper) so its calls are type-checked.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `python/structlog/constant-event` | structlog events are constant strings with the data as key/value pairs (log.info("user_login", user_id=uid)), not an f-string, .format() or %-formatted message: the event is what logs are grouped and searched by. | python | ast-grep forbid (where `$L`: types engine, §15) |
| `python/structlog/typed-logger` | structlog.get_logger() returns Any: assign it to an annotated name (log: structlog.typing.FilteringBoundLogger = structlog.get_logger()), or use structlog.stdlib.get_logger() with the stdlib wrapper, so its calls are type-checked. | python | ast-grep advise<br>advice, never blocks |

### python/temporal

Temporal workflows and activities (temporalio).

Detected by: a dependency on `temporalio` (PyPI).
Child of `python`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- Temporal (temporalio): workflow code is deterministic. Inside `@workflow.defn` classes, time, randomness and ids come from `workflow.now()`, `workflow.time()`, `workflow.random()` and `workflow.uuid4()`, waits from `asyncio.sleep` / `workflow.sleep`, `workflow.wait()` and `workflow.as_completed()`; I/O (HTTP, files, environment variables, threads) runs in activities.
- Every `workflow.execute_activity()` / `start_activity()` call (local, `_method` and `_class` variants too) passes `start_to_close_timeout=` or `schedule_to_close_timeout=`.
- The workflow sandbox stays on: `sandboxed=False`, `UnsandboxedWorkflowRunner()` and `workflow.unsafe.sandbox_unrestricted()` are the user's decision.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `python/temporal/activity-timeout` | workflow.execute_activity(), start_activity() and their local, _method and _class variants pass start_to_close_timeout= or schedule_to_close_timeout= (without one the call fails when the workflow runs). | python | ast-grep forbid (where `$F`: types engine, §15) |
| `python/temporal/no-sandbox-escape` | The workflow sandbox stays on. No @workflow.defn(sandboxed=False), UnsandboxedWorkflowRunner() or workflow.unsafe.sandbox_unrestricted(); escaping it is the user's decision: ask them first. | python | ast-grep forbid (where `$F`, `$D`: types engine, §15) |
| `python/temporal/workflow-determinism` | Workflow code is deterministic: inside a @workflow.defn class, no datetime.now/utcnow/today, time.time/monotonic/sleep, random, uuid1/uuid4, os.environ/getenv, open(), threading, asyncio.to_thread/wait/as_completed, run_in_executor or HTTP clients (httpx, requests, aiohttp). Use workflow.now(), workflow.time(), workflow.random(), workflow.uuid4(), asyncio.sleep or workflow.sleep, workflow.wait() and workflow.as_completed(), and activities for I/O (replay fails otherwise). | python | ast-grep forbid (where `$D`, `$F`, `$ENV`: types engine, §15) |

### python/torch

PyTorch models and their loading.

Detected by: a dependency on `torch` (PyPI).
Child of `python`: active only where it is.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `python/torch/no-unsafe-load-env` | TORCH_FORCE_NO_WEIGHTS_ONLY_LOAD is not set (Python, .env files, Dockerfiles, Compose files, CI workflows, shell scripts, config files): it turns torch.load's weights_only protection off for the whole process. | any: `**/*.py`, `**/.env`, `**/.env.*`, `**/Dockerfile`, `**/Dockerfile.*`, `**/*.Dockerfile`, `**/compose*.y*ml`, `**/docker-compose*.y*ml`, `**/*.sh`, `**/*.toml`, `**/*.ini`, `**/*.cfg`, `.github/workflows/*.y*ml` | grep forbid `TORCH_FORCE_NO_WEIGHTS_ONLY_LOAD` |
| `python/torch/no-unsafe-load` | No weights_only=False (torch.load, and the libraries that pass it on to torch.load): the file is then unpickled, which runs any code it holds. Load a state_dict, or allow the checkpoint's own classes with torch.serialization.add_safe_globals(). | python | ast-grep forbid |
| `python/torch/save-state-dict` | torch.save() a model's state_dict(), never the nn.Module itself (a pickled module loads only with weights_only=False, which runs code from the file); safetensors.torch.save_file() for weights to share. | python | ast-grep advise (where `$F`, `$M`: types engine, §15)<br>advice, never blocks |

### python/tortoise

Tortoise ORM models.

Detected by: a dependency on `tortoise-orm` (PyPI).
Child of `python`: active only where it is.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `python/tortoise/no-generate-schemas` | No Tortoise.generate_schemas() and no generate_schemas=True (register_tortoise, RegisterTortoise) outside tests. Tortoise 1.0 ships migrations; create the schema with tortoise makemigrations and tortoise migrate. | python: `!**/tests/**`, `!**/test_*.py`, `!**/*_test.py`, `!**/conftest.py` | ast-grep forbid (where `$G`, `$F`: types engine, §15) |
| `python/tortoise/primary-key-kw` | Tortoise fields take primary_key=True, not the deprecated pk=True (a DeprecationWarning at run time that the type checker does not report). | python | ast-grep forbid (where `$C`: types engine, §15) |

### python/typer

Typer command-line interfaces.

Detected by: a dependency on `typer` (PyPI).
Child of `python`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- Typer. Parameters are declared with `Annotated`: `name: Annotated[str, typer.Option(help=...)] = "x"`, never `name: str = typer.Option(...)`; the default is the parameter's default value.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `python/typer/annotated-params` | Typer parameters use Annotated (name: Annotated[str, typer.Option()] = "x"), not a typer.Option() / typer.Argument() default value. | python | ast-grep forbid |

### python/uv

uv is the Python package manager.

Detected by: the fact `python.pm` is `uv`.
Child of `python`: active only where it is.

Context (in `.claude/rules/forge-*.md`, §7):

- The project is managed with uv: dependencies are added with `uv add` (`uv add --dev` for tooling), and tools run through the project's environment (`uv run <tool>`).

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `python/uv/dependency-groups` | Development dependencies live in [dependency-groups] (PEP 735, uv add --dev), not in the deprecated [tool.uv] dev-dependencies. | any: `**/pyproject.toml` | grep forbid `^[[:space:]]*dev-dependencies[[:space:]]*=` |
| `python/uv/exclude-newer` | uv resolves only versions published at least 3 days ago ([tool.uv] exclude-newer set to a relative duration such as "7 days") and requires uv >= 0.9.17, which understands relative values. | any | check lint: `forge_uv_cooldown` (when `pyproject.toml`, `uv.toml` or `uv.lock` changes) |
| `python/uv/lock-sync` | uv.lock matches pyproject.toml. | any | check lint: `forge_do uv lock-check` (when `pyproject.toml` changes) |
| `python/uv/no-pip` | Dependencies are added with uv add and the environment is uv's; Claude asks before other package managers (poetry, pipenv, pdm, conda) or a hand-made virtualenv. | any | guard ask: command `{name: [poetry, pdm], sub: [add\|install]}` or `{name: pipenv, sub: [install]}` or `{name: [conda, mamba, micromamba], sub: [install]}` or `{name: [venv, virtualenv]}` |
| `python/uv/no-requirements-txt` | Claude asks you before writing or editing requirements*.txt in a uv project; dependencies go through uv add, and uv export writes a requirements file for consumers that need one. | any | permission ask `Write(**/requirements*.txt)`<br>permission ask `Edit(**/requirements*.txt)` |

### python/uvicorn

Uvicorn, run directly or as Gunicorn workers.

Detected by: a dependency on `uvicorn` (PyPI).
Child of `python`: active only where it is.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `python/uvicorn/forwarded-allow-all` | Uvicorn and Gunicorn do not trust X-Forwarded-* headers from every client (--forwarded-allow-ips "*", forwarded_allow_ips="*", FORWARDED_ALLOW_IPS=*): any client could then set its IP and scheme. List the proxy's addresses; when the app is only reachable through a trusted proxy, ask the user. | any: `@dockerfile`, `@procfile`, `@compose`, `@env-files`, `@gunicorn-config`, `**/*.py` | grep forbid `forwarded[-_]allow[-_]ips["']?[[:space:]]*[=:, ][[:space:]]*["']?\*` (ignoring case) |
| `python/uvicorn/no-legacy-ws-impl` | Uvicorn's WebSocket implementation is not the deprecated legacy one (--ws websockets, ws="websockets", UVICORN_WS=websockets); keep the default or use websockets-sansio. | any: `@dockerfile`, `@procfile`, `@compose`, `**/*.py` | grep forbid `--ws(=\|["',[:space:]]+)["']?websockets(["'[:space:]]\|$)\|(^\|[^A-Za-z0-9_])ws[[:space:]]*=[[:space:]]*["']websockets["']\|UVICORN_WS[[:space:]]*[=:][[:space:]]*["']?websockets(["'[:space:]]\|$)` |
| `python/uvicorn/uvicorn-worker` | Gunicorn runs Uvicorn through uvicorn_worker.UvicornWorker (package uvicorn-worker), not the deprecated uvicorn.workers.UvicornWorker; or Uvicorn runs alone with --workers N. | any: `@dockerfile`, `@procfile`, `@compose`, `@gunicorn-config`, `**/*.py` | grep forbid `uvicorn\.workers\.Uvicorn(H11)?Worker` |

### python/websockets

The websockets library.

Detected by: a dependency on `websockets` (PyPI).
Child of `python`: active only where it is.

| Rule | Description | Scope | Enforced by |
|---|---|---|---|
| `python/websockets/no-legacy-asyncio` | No websockets.legacy imports: the legacy asyncio implementation is deprecated; use websockets.asyncio.server.serve and websockets.asyncio.client.connect. | python | tool ruff `TID251` |
