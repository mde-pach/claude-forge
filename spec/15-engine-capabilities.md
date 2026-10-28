## 15. Engine capabilities

A rule says **what** it needs to know about the code; the engine decides **how** to find it out. A capability is a question a rule asks declaratively in its YAML, answered by **one pinned, provisioned implementation per language** (§15.2). Packs and rules never name the tool that answers: replacing ty or TypeScript's server by another type checker changes the engine, never a rule.

- A capability MUST NOT decide on names the project chose (variables, receivers, helpers, parameters, free text). Literal conventions of a tool or library are allowed (a library's own API names, `Dockerfile`, `*_TOKEN` keys in `.env`), and each one is overridable in `forge.yaml` (§15.4).
- There is no fallback. A capability that cannot answer is reported with the rules it leaves unchecked, never taken as a pass and never replaced by a weaker heuristic (§15.1).

| Capability | Asked from | Answers | State |
|---|---|---|---|
| types | `where: {<VAR>: {type: …}}` on an `ast-grep` target | what the static type of the expression bound to a metavariable is declared as | implemented (§15.1–§15.3) |
| declared | `where: {<VAR>: {declared: …}}` | what a name refers to (an imported function or class, through aliases): `textDocument/definition` instead of `typeDefinition`, no supertypes | implemented (§15.6) |
| deprecated | `where: {<VAR>: {deprecated: true}}` | whether the name is a use of a declaration marked `@deprecated`, as the project's TypeScript reports it | implemented (§15.7), TypeScript |
| files | `@<role>` in `scope.files`, `when_changed`, check commands and a where `module` | the paths of a file role: Forge's defaults, the tool's declarative config, `forge.yaml` | implemented (§15.4) |
| tests | engine helpers of test checks | the tests a runner collects and the ones that failed, from the runner's own reports | proposed (§15.5) |

### 15.1 Types

```yaml
# packs/python/sqlalchemy/rules/no-query-api.yaml
enforce:
  - target: ast-grep
    mode: forbid                    # forbid or advise
    rule: {pattern: "$S.query($$$)"}
    where:
      S: {type: {package: sqlalchemy, name: [Session, scoped_session, AsyncSession, async_scoped_session]}}
examples:
  deps: [{ecosystem: pypi, name: sqlalchemy, version: "2.0.44"}]
  match: ["from sqlalchemy.orm import Session as Db\n\n\ndef first(db: Db):\n    return db.query(int).first()\n"]
  no_match: ["class Session:\n    def query(self, q: str) -> list[str]:\n        return [q]\n\n\ndef search(session: Session):\n    return session.query(\"text\")\n"]
```

`where` maps single metavariables of the rule (`$S`, not `$$$S`) to a condition: `{type: <spec>}`, `{declared: <spec>}` (the declaration a name refers to, §15.6), `{deprecated: true}` (a deprecated use, §15.7) or `{not: <condition>}`. A spec has `package` and/or `module` (a string or a list; at least one: a name alone would be a guess), optionally `name` (a string or a list of identifiers) and `unknown: match | no-match` (default `no-match`).

**Semantics.** For each match, each condition is evaluated on the expression the metavariable is bound to; the match counts only when all of them hold.

1. **The expression's type** is what the language's type checker infers for exactly that expression, in the file as it is on disk: the engine shows the server, in memory only, the file with the expression bound to a fresh name (Python `(__forge_tN := (<expr>))`, TypeScript `((__forge_tN = (<expr>)) => __forge_tN)()`), asks `textDocument/typeDefinition` at that name, then shows it the file as it is. Any expression works (a name, an attribute, a call such as `sessionmaker(e)()`, a subscript). Nothing is written to the project.
2. **Declarations.** Each location returned is the declaration of the type, or of one member: a union or intersection gives one per member (`Session | None`: `Session` and `NoneType`). Import aliases, re-exports and type aliases are resolved by the checker: a declaration has the name it is declared with (`Session`, never the alias `Db`). An anonymous type (an object type literal, a function or tuple type) is named by the type alias that declares it (`type UseFormWatch<T> = { … }` is `UseFormWatch`, never a word of the literal or its comments); one written inline has no name. An intersection has no declaration of its own, and TypeScript's servers give none for it or for an alias of one: then the engine reads the inlay hint of the bound name's type, which is the type as the checker prints it (an alias by its name), each name in it linked to the declaration it refers to. The names at its top level give the declarations: the type it names, or each member of a union or intersection, through grouping parentheses; never a type argument, an object or function type, a `typeof` or `keyof` operand, an indexed access or an array. `drizzle()`'s `NodePgDatabase<…> & { $client: Pool }` is `NodePgDatabase` (the literal member has no name), zustand's `create()()` is `UseBoundStore` (`type UseBoundStore<S> = { … } & S`), node-redis's `createClient()` is `RedisClientType`, Fastify's `Fastify()` is `FastifyInstance` and `PromiseLike`, Pulumi's `Output<T>` is `Output`. No name of the project's choosing is read: the printed type and its links are the checker's.
3. **Subtypes.** Each declaration counts with its supertypes, transitively, at most 64 (Python: the server's type hierarchy; TypeScript: the classes and interfaces in its `extends` and `implements` clauses, resolved with `textDocument/definition`, and for a type alias the types its body is made of, read as in rule 2: the type it renames, the members of its union or intersection; a conditional type's branches depend on its arguments and are not followed). `RedisClientType = (RedisClient<…> & RedisClientExtensions<…>)` is a `RedisClient`; Prisma's generated `Prisma__UserClient<T> extends Prisma.PrismaPromise<T>`, where `PrismaPromise<T>` is an alias of the runtime's `PrismaPromise<T>`, is a `PrismaPromise`. A project class deriving from `Session` is a `Session`. Assignability is nominal: a structurally compatible type that does not derive from the declaration does not count.
4. **Generics.** Type arguments are ignored: `list[Session]` is `list`, `scoped_session[Session]` is `scoped_session`.
5. **Where a declaration is.** Its file decides. Python: under `site-packages` / `dist-packages`, the package is the first path segment (`<pkg>-stubs` counts as `<pkg>`) and the module the dotted path (`sqlalchemy.orm.session`, `__init__` dropped); typeshed's `stdlib/` and `stubs/<dist>/` likewise, and the standard library's own sources (`lib/python3.X/`, where ty's definitions of stdlib functions go: `random.choice` is `random`). TypeScript: under the innermost `node_modules/`, the package is `<name>` or `@scope/name` (`@types/x` counts as `x`, `@types/a__b` as `@a/b`) and the module `<package>/<path in it without extension>` (`zod/v4/classic/schemas`); TypeScript's own `lib.*.d.ts` are package `typescript`. A declaration in the project has no package; its module is its path (Python: dotted, without a leading `src.`). The package is the one that **declares** the type, which may be a library's internal package (Vitest's `expect` is declared in `@vitest/expect`).
6. **A spec holds** when one declaration or supertype has the package (one of the list), the module (equal, or a prefix by whole segments: `.` in Python, `/` in TypeScript) and the name (one of the list) that the spec gives. With a union, one member is enough: the expression may hold such a value at run time. A condition may give a list of specs (`type: [{package: datetime, name: now}, {package: time, name: time}]`): it holds when one of them does.
7. **Unknown types.** When the checker does not know the type (Python `Any` and `Unknown`: an unannotated parameter, an unresolved import; TypeScript `any`, `unknown`, an error: no declaration), a spec does not hold, unless it says `unknown: match`. Type constraints therefore rely on typed code, which the type-checking rules enforce: `python/no-unknown` (ruff `ANN001`–`ANN003`, `ANN201`, `ANN202`: annotated parameters and returns) and `python/no-explicit-any` (`ANN401`); `typescript/no-implicit-any` (`noImplicitAny`) and `typescript/no-explicit-any`. Removing an annotation to escape a rule is then itself a finding.
8. `not` inverts its condition. A metavariable the match did not bind (an `any:` branch without it) is not evaluated.
9. **Code that is not an expression.** A metavariable bound to code that cannot be bound as an expression (a parameter's or a keyword argument's name, a type) makes the file unparsable once bound: Python, the file with that binding does not parse (Python's `ast`) while the file itself does; TypeScript, the server reports a syntax error (TypeScript's diagnostics 1000–1999) in the code the binding inserts, when the type has no declaration. Its type cannot be asked: the match is reported `forge:forge/types <rule-id> was not checked: <file>:<line>: $<VAR> is bound to `<code>`, which is not an expression: its type cannot be asked` (feedback after an edit, a failure in check.sh), never passed; the file's other expressions are asked without that binding.

**Stubs.** A library whose types the checker sees only through a stub package (`django-stubs`, `celery-types`, `asyncpg-stubs`) is named in the target's `needs: [{ecosystem, name}]`: where the rule applies (§5.1) and the project does not declare that package, the target is not rendered and `forge status` reports `not enforced here: the types engine needs <package> installed …`. Its `examples.deps` must install it.

**Where it runs.** A rule's `where` is rendered into its ast-grep rule file as `metadata: {forge_where: …}`, so each match carries it (`ast-grep scan --include-metadata`). post-edit.sh (§8.1) and check.sh step 3 (§10.1) keep the matches on added lines, then pass those with `forge_where` to `forge_types_filter` (engine.sh), which asks the language's engine and keeps the matches it confirms; other matches are untouched. An engine that cannot start or answer prints `forge:forge/types <rule-id> was not checked: <reason>`: after an edit as feedback, in check.sh as a failure.

**Schema and `forge test`.** `where` is allowed on `forbid` and `advise` targets (counted modes would need the base copy's types: not yet), with `scope.language` `python`, `typescript` or `tsx`, on metavariables that occur in the rule. Such a rule's `examples.deps` lists the packages its examples are type-checked against (its language's ecosystem, exact versions); a rule whose types come only from the standard library (Python's, which ty ships) or TypeScript's own `lib.*.d.ts` lists none. Its examples are whole files (imports included). Its ast-grep test proves the `match` examples structurally; then every example is scanned and filtered by the generated `forge_types_filter` in a project holding those packages (none: the standard library or TypeScript alone; a uv environment on the engine's Python, or a `bun install`ed `package.json` whose `tsconfig.json` lists the `@types/*` packages among them in `types`, as a project using their globals does: TypeScript 6 and later include none by default; kept in `$FORGE_EXAMPLES_CACHE`, default `~/.cache/forge/examples/`): a `match` example must keep a match, a `no_match` example none.

### 15.2 Engines

| Language | Server | Pin | Started as | Adapter |
|---|---|---|---|---|
| Python | `ty server` (Astral) | ty 0.0.86 | `uvx --python 3.12 --from ty==0.0.86 python <adapter> <root> <state>` | `types_python.py`: standard library only, on the Python uvx provides next to the pinned `ty` |
| TypeScript | the project's own TypeScript (`node_modules/typescript`, resolved from the root): 7 and later, its language server (`bin/tsc --lsp --stdio`, the Go compiler); 5 and 6, its `lib/tsserver.js` (tsserver protocol) | the project's (its lock file) | `bun <adapter> <root> <state>` | `types_typescript.mjs`: Node-compatible, no Bun-only API |

- **Installed packages without typing are Any.** ty infers the types of an installed package that ships no typing (PEP 561: no `py.typed`, no `.pyi`, no `<name>-stubs` distribution) from its source, where mypy treats it as `Any`; on a library with runtime magic that rejects correct code (SQLAlchemy 1.3's `x = Column(Boolean)` then `obj.x = True`: not assignable to `Column`). Unless `forge.yaml` says `python: {untyped-packages: source}`, ty runs with those modules in `analysis.replace-imports-with-any` (`<name>` and `<name>.**`), given at run time only (`ty check -c …`; `ty server`'s inline `configuration` setting), never written into the project's config; ty adds them to the list the project's config may already set, whose entries stay.
  - **Which.** `forge_ty_untyped` (types.sh) takes the environment ty checks with (`environment.python` of its config, `$VIRTUAL_ENV`, `.venv`, else §9.1's lookup) and its site-packages directories (the interpreter's `sys.path` entries named `site-packages` / `dist-packages`, as `python -I` lists them: no user site, no `PYTHONPATH`). `.forge/helpers/untyped_python.py`, on the pinned ty's Python, lists their modules: a package (a directory with `__init__.py(i)`), a module (`.py`, an extension module), and a namespace package by portion (`google.cloud`, not `google`: each portion is its own distribution's); the first directory holding a name wins, as for imports. A module is typed when its package holds `py.typed` or any `.pyi`, a `.pyi` sits next to it, or a `<top>-stubs` package covers it in any of the directories.
  - **Not replaced.** A module ty resolves without that environment: one of the standard library stubs ty bundles (`typing_extensions`: a backport named like the module; replacing it makes ty 0.0.86 panic, `Typeshed should always have a tuple class in builtins.pyi`), and the project's own code (an installed distribution may shadow a first-party name: a site-packages `tests` package next to the repository's `tests/`). Which ones they are is asked from the pinned ty, not listed by Forge: each candidate is imported and revealed (`import <name> as _m; reveal_type(_m)`) in a probe file checked with the project's configuration (its search roots, its Python version, else the environment's) and an empty environment; one that reveals `<module '…'>` resolves without the environment and is left out.
  - **Kept, and self-updating.** The list is a fact (§9.4) keyed on the interpreter, the pins, the helper's text and the project's `pyproject.toml` / `ty.toml`, and watching the site-packages directories (each distribution's `*.dist-info` / `*.egg-info` name carries its version) and ty's first-party search roots (`environment.root`, else `.`, `src`, `python`, `tests`). A package upgraded to a release that ships typing changes its `dist-info` name, so the list is computed again and the package leaves it; nothing is done by hand. Computing it takes about 0.5 s (190 distributions); the cached answer, a directory listing and a hash per directory.
  - **The same answer everywhere.** `forge_ty` (`python/typecheck` at Stop, `--changed`, commit, CI; the edit-time typecheck), the `tool: ty` audits and `forge status` (which shows the count and the first names, §3) run the same function; in the base tree of §10.1 step 4, `forge_ty` uses the working tree's environment and list. The types engine's `ty server` gets the list as inline configuration (`FORGE_TY_ANY` to the adapter), and the list's hash is part of the engine's `want` (§15.3): every hook that asks the engine computes the list first, and a daemon started with another list is restarted. One exception, on purpose: the engine never gets as Any a package the active rules' `where` conditions name (`package`, or a `module`'s top-level package: `FORGE_TYPES_PACKAGES`), typed or not: those rules ask ty what the package declares (Django's `ForeignKey`, SQLAlchemy 1.x's `Session`), which an `Any` hides, and a rule that cannot see its package matches nothing (§15.1 rule 7). `ty check` keeps them as Any: there the package's own inferred types are the false errors (SQLAlchemy 1.x columns), while the engine only reads where a name is declared and what a receiver's type is.
- **ty is Python's type checker in Forge, everywhere**: the types engine, `python/typecheck` (`forge_do ty typecheck`: ty's adapter, §16, runs `forge_ty`) and the `tool: ty` audits run the same pinned ty through uvx, with the project's environment and configuration. basedpyright is not used.
- ty's pin follows Forge's rule for its own tools (the newest release older than 14 days; ast-grep, Squawk, zizmor, hadolint) and is embedded in the generated scripts with a revision of the adapters.
- `uvx` (Python) and `bun` (TypeScript) are required tools of the capability: missing, they are listed by `forge_missing_tools` (SessionStart, `forge status`). `forge apply` provisions the pinned ty (fills the uvx cache), so the hooks run offline afterwards. TypeScript's engine is the project's: the version that type-checks the project answers its type questions, and a project without `node_modules/typescript` gets `forge:forge/types <rule> was not checked: the project has no typescript package …`. The same semantics and the same spec tests hold for TypeScript 5, 6 and 7 (one version of each is pinned in the test tools).
- The engine answers with the project's own environment: ty finds `.venv` (or `VIRTUAL_ENV`) and the project's `[tool.ty]`; any other environment of the project (§9.1: uv's `UV_PROJECT_ENVIRONMENT`, Poetry's, the interpreter on `PATH` a project is installed into) is given to ty, which would otherwise take the Python uvx puts first on `PATH`, its own: `ty check --python <its sys.prefix>` (`forge_ty`, `python/typecheck`, the `tool: ty` audits) and, to `ty server`, the active environment of its workspace settings (`pythonExtension.activeEnvironment`: the interpreter and its `sys.prefix`, `FORGE_TY_PYTHON` to the adapter). TypeScript reads the project's `tsconfig.json` files and `node_modules`, workspace packages included.
- Both adapters implement one protocol (§15.3) and the semantics of §15.1; only the way supertypes are found differs (ty has a type hierarchy; TypeScript's servers have none), and TypeScript's servers name an intersection through the inlay hint of the binding's parameter (tsserver `provideInlayHints` with `interactiveInlayHints`, `tsc --lsp` `textDocument/inlayHint` with `inlayHints.parameterTypes`), asked only when `typeDefinition` gives nothing. The TypeScript binding `((__forge_tN = (<expr>)) => __forge_tN)()` declares no name outside the arrow (an arrow keeps `this`; an `await` in the expression is still typed), so the server keeps that version of the file until it changes: one program update per question, and one more for each binding the parser rejects (§15.1 rule 9). To tsserver only the span that differs is sent.
- **Loading.** The TypeScript adapter loads the project's tsconfig projects (one tracked source file of each `tsconfig.json`) in the background once ready, questions first; while it does, `<state>/types/typescript/loading` exists. After an edit, a question to an engine still loading waits at most `FORGE_TYPES_LOADING_WAIT` seconds (5) and then prints `forge:forge/types <rule-id> was not checked after this edit: the typescript engine (<server>) is still loading the project; the Stop check runs it`; the Stop check and CI wait for the answer. SessionStart starts the engines, so the load usually ends before the first edit.

**Evidence** (Apple M2, 2026-10-26; `typeDefinition` on a receiver; cold = spawn to `initialize` reply; first = `didOpen` + first answer; warm = `didChange` + answer, median of 20; RSS after warm-up):

| Project | Server | Cold | First | Warm | RSS |
|---|---|---|---|---|---|
| fastapi-app sample | ty 0.0.92 / basedpyright 1.40.2 | 8 / 171 ms | 42 / 478 ms | 0.5 / 11.5 ms | 43 / 354 MB |
| Netflix dispatch (655 files, SQLAlchemy 2.0.8) | ty 0.0.86 / 0.0.92 / basedpyright | 8 / 8 / 181 ms | 226 / 196 / 1338 ms | 7.5 / 6.0 / 4.3 ms | 103 / 100 / 517 MB |
| nextjs-prisma-app sample | TypeScript 7.0.2 / typescript-language-server + 5.9.3 | 45 / 65 ms | 41 / 924 ms | 1.0 / 3.5 ms | 103 / 782 MB |
| documenso (2245 TS files, workspaces) | TypeScript 7.0.2 / typescript-language-server + 5.9.3 | 42 / 63 ms | 408 / 4538 ms | 86 / 97 ms (p95 163 / 122) | 1642 / 1998 MB |

Through the TypeScript adapter on documenso (12 tsconfig projects; its own TypeScript 5.6.2, then 6.0.3 and 7.0.2 in its `node_modules`; a question on `packages/lib/utils/document-audit-logs.ts` with 2 matches of a `{package: zod}` rule; warm = after a `changed` line for an edit of that file, median of 10):

| TypeScript | Server | Ready | A question during the load | Load of the 12 projects | Warm question |
|---|---|---|---|---|---|
| 5.6.2 | tsserver | 400 ms | 9.5 s | 21.3 s | 145–152 ms (max 217) |
| 6.0.3 | tsserver | 272 ms | 7.0 s | 14.5 s | 145–153 ms (max 209) |
| 7.0.2 | tsc --lsp | 91 ms | 0.7 s | 1.2 s | 39 ms (max 71) |

tsserver's warm cost is its own program update after an edit in a large project (sending only the changed span, or one update instead of three per question, changed nothing measurable). The fixture of §15.1's tests answers 16 matches in 95–99 ms (5.9.3, 6.0.3) and 33 ms (7.0.2) cold, with the same 12 matches kept.

Through the adapters and the FIFO: a question from a POSIX `sh` client costs 5.6 ms of client and transport (bash 3.2 as `sh`); the engine answers a warm question in 2–16 ms (1 to 14 matches), the cold one in 63–250 ms; the whole `forge_types_filter` of an edit takes about 50 ms, sourcing the script included. ty and TypeScript 7 answer correctly through import aliases, re-exports, `sessionmaker()` results, subscripts, unions with `None`, subclasses (with the supertype walk) and generics, and give no declaration for `any`; queried at the end of a call or subscript, basedpyright (measured for the choice; Forge no longer uses it) and tsserver answer nothing, but through the binding of §15.1 (a name) tsserver answers every case like TypeScript 7.

### 15.3 Daemons

- **One daemon per language and repository**, in `<state>/types/<lang>/` (the hook state outside the repository, §8): `in` (the request FIFO), `started` (FIFO, start reports), `pid`, `ready` (the server's name and version), `want` (engine pin and adapter revision), `log`, `sessions/`. The adapters are `.forge/helpers/types_python.py` and `.forge/helpers/types_typescript.mjs`.
- **Start** (`forge_types_up <lang>`): SessionStart starts the engines the active rules use, in the background; any hook that needs an answer starts its engine when it is not running (the same function: there is no other path). A lock directory holding the starter's pid serialises starters; the adapter detaches into its own session (the hook that started it may end) and writes `ready …` or `failed …` to `started`; a launcher failing before it (a first download without network) gives `exited <code>`. A daemon whose `want` differs from the scripts' (another pin, another Forge, for Python another list of untyped packages, §15.2) is stopped and started again. Starting takes at most `FORGE_TYPES_START_TIMEOUT` seconds (60).
- **Protocol.** Lines written to `in` are shorter than `PIPE_BUF`, so concurrent writers never interleave, and `in` is opened read-write, so a writer never blocks: `changed<TAB><path>`, `stop`, `restart`, or the absolute path of a question directory, whose `req` holds ast-grep match lines; the `reply` holds the matches whose `forge_where` holds, `error<TAB><why>` (the question failed: its rules are not checked), `unchecked<TAB><rule-id><TAB><why>` per match the engine cannot check (§15.1 rule 9), then `end`. The client opens the question's `reply` FIFO read-write before asking (the daemon's non-blocking open always finds a reader), reads up to `end` and gives up after `FORGE_TYPES_TIMEOUT` seconds (20). Questions are answered one at a time, in order; parallel hooks queue.
- **What the server sees.** The asked file is read from disk at each question and sent (`didOpen`, then `didChange` with the full text) when it differs from what the server has. Other files reach the server through `changed` lines: post-edit.sh sends every file of the edit (Edit, Write, the files a Bash command changed), turn-start.sh the uncommitted files and those a commit, checkout or pull changed since the previous turn, check.sh its change set. A change to a lock file, `pyproject.toml`, `ty.toml`, `package.json` or `tsconfig*.json` restarts the daemon instead (`restart`): the next question starts it on the new environment. Nothing is written in the repository.
- **End.** Sessions register when they start the engine or find it running; SessionEnd (`session-end.sh`) unregisters, and the last session to end stops it. A stop is synchronous (`stop<TAB><dir>`: the daemon answers once its language server has exited), so check.sh outside a session (pre-commit, CI), which stops the engines it started, leaves nothing running when it exits. A daemon also exits after `FORGE_TYPES_IDLE` seconds (1800) without a line, and within 10 seconds of its state directory or the repository disappearing, or of another daemon taking its state directory over (its `pid` file names the other one: a start whose starter was killed can race with the next one), whether lines keep coming or not. The start lock holds the starting shell's own pid (not `$$`, which a background subshell outlives), and a starter that gets the lock checks again whether the engine started meanwhile, so concurrent hooks start one engine.

### 15.4 File roles

Rules and helpers name the files they are about by **role**, not by literal globs: `scope.files: ["@python-tests"]`, `"!@alembic-migrations"`, `when_changed: ["@compose"]`, `forge_role @<role> <path>` in check commands, `module: "@wrangler-types"` in a where type spec. Each role has Forge's defaults; `forge.yaml` changes them:

```yaml
files:
  python-tests: {add: ["integration/**/check_*.py"]}   # added to the defaults (or to the tool's config)
  env-files: {set: [".env", ".env.local"]}             # replaces the defaults and the tool's config
```

- **Effective patterns** are computed by `forge apply` and rendered literally into the generated files: a rule's `scope.files` and `when_changed` (shell `case` patterns, ast-grep `files` / `ignores`), and `forge_role` in the checks fragment (the roles the active rules name, and those engine helpers name: `@dockerfile`, `@alembic-migrations`). Hooks never parse configs. `forge status` prints a `Files:` section: each role an active rule names, its patterns and where each comes from (`forge default`, the tool's config file, `forge.yaml`), and `matches no file here` for a role no file matches (`--json`: `files`). The lock records each such role's patterns (`roles`); a changed source is drift (`forge status`, `forge diff`: `files @<role> changed since the last apply: …`).
- **In rules.** `@<role>` in a glob list stands for the role's patterns (its exclusions included); `!@<role>` excludes its files (a role with exclusions cannot be negated). A role a rule names that Forge does not know is an error when the rule is loaded. A rule whose file selection (`scope.files`, or a `when_changed`) is only roles with no pattern is not enforced, and `forge status` says which role to set: it never runs on other files. A where `module` naming a role (`@wrangler-types`) stands for the modules of the role's files, as §15.1 names a project file's module (TypeScript: the path without its extension; Python: dotted); such a role lists files, not globs.
- **Check helpers.** `forge_role @<role> <path>` succeeds when the repository path is one of the role's files; `forge_role_files @<role>` prints the repository's files of the role (tracked, and untracked but not ignored); `forge_applied_migrations @<role>…` reports migrations of the roles that exist in the base and were modified or deleted (not with `--all`). In a check command a role is an unquoted `@<role>` word; quoted text (`grep -q '@apply'`) is the command's own.
- **Defaults from the tool's own config**: only when the config is data Forge reads without running code (INI, TOML, JSON, `schema.prisma`, `.adr-dir`, `atlas.hcl` `dir`), read at the repository root, and then they replace Forge's defaults for that role (for `@sql-migrations`, the default of that tool only). What a config says that Forge cannot use as a path (an Alembic package resource `pkg:dir`, a Flyway `classpath:` location) is shown under the role by `forge status`, and the defaults stay. Code configs (`vitest.config.*`, `playwright.config.*`, `drizzle.config.*`, `prisma.config.ts`, `kysely` migrators) are never evaluated: the tool's documented defaults apply, and the user overrides in `forge.yaml`.
- **Roles and defaults:**

| Role | Defaults | From the tool's config |
|---|---|---|
| `adr` | `docs/adr/**`, `doc/adr/**`, `docs/decisions/**` | adr-tools `.adr-dir` + `/**` |
| `alembic-migrations` | `alembic/versions/**`, `migrations/versions/**` | `alembic.ini` (or `[tool.alembic]`) `version_locations`, else `script_location` + `/versions/**` |
| `angular-tsconfig` | `**/tsconfig.app.json` | `angular.json`: each project's `architect` (or `targets`) `build.options.tsConfig` |
| `better-auth-config` | the auth CLI's search paths: `{,src/,app/}{,lib/server/,server/auth/,server/,auth/,lib/,utils/}{auth,auth/index}{.ts,.tsx,.js,.jsx,.server.js,.server.ts}` | — |
| `build-artifacts` | `**/node_modules/**`, `**/__pycache__/**`, `**/*.pyc`, `**/.next/**`, `**/.nuxt/**`, `**/.svelte-kit/**`, `**/.turbo/**`, `**/.coverage`, `**/.DS_Store` (and, in `forge_artifacts`, any directory holding `pyvenv.cfg` or `CACHEDIR.TAG`) | — |
| `compose` | `**/compose*.yaml`, `**/compose*.yml`, `**/docker-compose*.yaml`, `**/docker-compose*.yml` | — |
| `django-migrations` | `**/migrations/[0-9]*.py` | — |
| `django-settings` | `**/settings.py`, `**/settings/*.py` | pytest-django `DJANGO_SETTINGS_MODULE` (`a.b` → `a/b.py`, `a/b/*.py`) |
| `django-templates` | `**/templates/**/*.html`, `**/templates/**/*.txt` | — |
| `dockerfile` | `**/Dockerfile`, `**/Dockerfile.*`, `**/*.Dockerfile`, `**/Containerfile`, `!**/*.dockerignore` | — |
| `docs` | `docs/**/*.{md,mdx,rst}`, `doc/**/*.{md,mdx,rst}`, `README*` | — |
| `drizzle-migrations` | `drizzle/**` | — (code config) |
| `e2e-tests` | `e2e/**`, `tests/e2e/**` | — (code config) |
| `env-files` | `**/.env`, `**/.env.*`, `!@env-template` | — |
| `env-template` | `**/.env.example`, `**/.env.sample`, `**/.env.template`, `**/.env.dist` | — |
| `esm-modules` | `**/*.mts`, `**/*.mjs` | plus `<dir>/**/*.{ts,tsx,js,jsx}` for each `package.json` with `"type": "module"`, less those under a nested `package.json` without it |
| `generated-clients` | none | `schema.prisma` generator `output` (relative to the schema) + `/**` |
| `gunicorn-config` | `**/gunicorn.conf.py` | — |
| `lockfiles` | `uv.lock`, `bun.lock`, `bun.lockb`, `package-lock.json`, `pnpm-lock.yaml`, `yarn.lock` | — |
| `prisma-migrations` | `prisma/migrations/**` | `package.json` `prisma.schema` directory + `/migrations/**` |
| `procfile` | `Procfile`, `Procfile.*` | — |
| `python-tests` | `**/test_*.py`, `**/*_test.py`, `**/conftest.py` | pytest `testpaths` × `python_files` (`pytest.ini`, `[tool.pytest]` / `[tool.pytest.ini_options]`, `tox.ini`, `setup.cfg`), plus `**/conftest.py` |
| `sql-migrations` | `db/migration/**/*.sql` (Flyway), `supabase/migrations/*.sql`, `db/migrations/*.sql` (dbmate) | Flyway `flyway.locations` `filesystem:` dirs (`flyway.conf`, `conf/flyway.conf`, `flyway.toml`) replace Flyway's; `deploy/**/*.sql` with `sqitch.plan` (under `sqitch.conf` `top_dir`); Atlas `atlas.hcl` `dir = "file://…"` + `/*.sql` (default `migrations/*.sql`) |
| `ts-tests` | `**/*.{test,spec}.{ts,tsx,mts,cts,js,jsx,mjs,cjs}`, `**/__tests__/**` | — (code config) |
| `typeorm-datasource` | none: set it in `forge.yaml` | — (code config) |
| `wrangler-config` | `**/wrangler.toml`, `**/wrangler.json`, `**/wrangler.jsonc` | — |
| `wrangler-types` | `worker-configuration.d.ts` | — (a CLI argument) |

- Patterns are globs: `**` spans directories, `*` and `?` one segment, `[0-9]` / `[!x]` one character, `{a,b}` alternatives. A role may include another (`!@env-template`); cycles are rejected, and so are roles `forge.yaml` names that Forge does not know.

### 15.5 Tests (proposed)

Test checks read the runner's own reports rather than its console output or ast-grep patterns: the tests a runner collects (`pytest --collect-only -q`, `vitest list --json`) give test identity for test integrity (§10.5) and failing tests come from its machine-readable report (pytest `--junitxml`, Vitest `--reporter=junit`) for §10.6, through engine helpers with the same interface as now. Pack rules keep naming the runner's checks; the helpers change.

### 15.6 Declared names

```yaml
# packs/python/numpy/rules/no-allow-pickle.yaml
enforce:
  - target: ast-grep
    mode: forbid
    rule:
      kind: call
      pattern: "$F($$$)"
      has: {field: arguments, has: {kind: keyword_argument, regex: "^allow_pickle[[:space:]]*=[[:space:]]*True$"}}
    where:
      F: {declared: {package: numpy, name: load}}
```

`{declared: <spec>}` holds when the name or attribute the metavariable is bound to **refers to** a declaration that the spec describes (§15.1 rule 6, the same `package`, `module`, `name` and `unknown`): for functions, decorators, classes and modules used by name (`np.load(…)`, `@workflow.defn`, `exec(…)`, `` $`…` ``), where `type` would describe the value's type: a function's type has no name, and TypeScript's type definition of a name bound to a function is the declaration of what it returns.

1. **The name.** The expression must end with a name: a name (`load`) or an attribute (`np.load`, `workflow.unsafe.sandbox_unrestricted`); its last name is asked. Any other expression (a call `pick()`, a subscript) is unknown (rule 4).
2. **The declaration** is what the language's checker answers to `textDocument/definition` at that name, in the file as it is on disk (TypeScript: in the version of §15.1 the server has, the position moved past the bindings). The checker resolves import aliases, namespace imports and re-exports, the project's included (`from numpy import load as read`, `import * as cp from "node:child_process"`, `export { useForm as useAppForm } from "react-hook-form"`): a declaration has the name it is declared with. Overloads and merged declarations each give one. No supertypes are added: a class's subclass is not that class.
3. **Where it is** decides as in §15.1 rule 5: the package and module of its file, the name it is declared with. A module (`import numpy as np` then `$F` bound to `np`) is declared by its file, with its module's last segment as name (Python).
4. **Unknown.** A name the checker gives no definition for (an attribute of `Any`, an unresolved name), one whose definition is an import the checker could not resolve (the definition stops at the project's import statement), and an expression that does not end with a name are unknown: the spec does not hold unless it says `unknown: match`.
5. A name bound to a project variable refers to that variable (`run = np.load`, then `run(…)`): the declaration is the project's, whatever its value. TypeScript's servers answer a called variable with its declarations and then the declaration of the signature it is called with (`useForm`'s for `const make = rhf.useForm; make()`, `NodeJS.Require`'s call signature for a module's own `const require = createRequire(…)`): that last one is what the value is, not what the name refers to, and is left out.

Same engines, daemons, protocol and `forge test` as §15.1: a rule may mix `type` and `declared` conditions on different metavariables. Through the adapters, ty, tsserver (TypeScript 5, 6) and `tsc --lsp` (7) answer `definition` on a name through aliases, namespaces and re-exports; an unresolved import gives the import itself or nothing, and an attribute of `any` / `Any` nothing.

### 15.7 Deprecated uses

```yaml
# packs/typescript/rules/no-deprecated.yaml
enforce:
  - target: ast-grep
    mode: forbid
    rule: {any: [{kind: identifier, pattern: $N}, {kind: property_identifier, pattern: $N}, {kind: type_identifier, pattern: $N}, {kind: shorthand_property_identifier, pattern: $N}]}
    where:
      N: {deprecated: true}
```

`{deprecated: true}` holds when the name the metavariable is bound to is a use of a declaration marked `@deprecated`, as the project's own TypeScript reports it (the strikethrough of an editor), in the file as it is on disk:

1. **Uses the checker reports.** TypeScript's suggestion diagnostics `6385` ("'X' is deprecated.") and `6387` ("The signature … of 'X' is deprecated."): a reference, a property access, a call or construction (for an overloaded function, the signature the call resolves to: a call of a non-deprecated overload is not one), a type, an import. The name holds when its range overlaps one of them. tsserver (TypeScript 5, 6) answers `suggestionDiagnosticsSync` and `tsc --lsp` (7) `textDocument/diagnostic`, one request per file and question.
2. **Object literal keys**, which TypeScript checks against the contextual type but never reports: a name in a key's position (`{ name: …`, `, name: …`, a shorthand `{ name }`) holds when the server documents the property as deprecated (tsserver's `quickinfo` kind modifiers; the language server's hover, which renders the tag as `*@deprecated*`) and its definition is not the name itself (an interface or type member declaring a deprecated property is not a use of it).
3. A declaration is never a use: the checker reports none on a deprecated function's or property's own name.

Only TypeScript's engine answers it: a Python rule with `deprecated` is rejected when it is loaded (ty's own `deprecated` rule covers Python, `python/no-deprecated`). The same spec tests hold for TypeScript 5, 6 and 7.
