## 15. Engine capabilities

A rule says **what** it needs to know about the code; Forge decides **how** to find it out. A capability is a question a rule asks in its YAML, answered by one implementation per language. Rules never name the tool that answers: replacing ty or TypeScript's server changes the engine, never a rule.

- A capability never decides on names the project chose (variables, receivers, helpers, parameters). A library's own API names and a tool's literal conventions (`Dockerfile`, `*_TOKEN` keys in `.env`) are allowed, and each file convention can be changed in `forge.yaml` (15.4).
- There is no fallback. A question the engine cannot answer leaves its matches unconfirmed and gives a note for the person naming the rules it leaves unchecked. It is never taken as a pass, never replaced by a weaker heuristic, and never a finding for the agent.

| Capability | Asked from | Answers |
|---|---|---|
| types | `where: {<VAR>: {type: …}}` on an `ast-grep` target | what the static type of the expression bound to a metavariable is declared as (15.1) |
| declared | `where: {<VAR>: {declared: …}}` | what a name refers to: an imported function, class or module, through aliases and re-exports (15.6) |
| deprecated | `where: {<VAR>: {deprecated: true}}` | whether a name is a use of a declaration marked `@deprecated`; TypeScript only (15.7) |
| files | `@<role>` in `scope.files`, `when_changed`, `config_files`, check commands and a where `module` | the paths of a file role (15.4) |

### 15.1 Types

```yaml
# packs/python/sqlalchemy/rules/no-query-api.yaml
enforce:
  - target: ast-grep
    mode: forbid                    # forbid or advise
    rule: {pattern: "$S.query($$$)"}
    where:
      S: {type: {package: sqlalchemy, name: [Session, scoped_session, AsyncSession, async_scoped_session]}}
```

`where` maps single metavariables of the rule (`S` for `$S`; a name of capitals, digits and `_`) to a condition: `{type: <spec>}`, `{declared: <spec>}`, `{deprecated: true}` or `{not: <condition>}`. A condition's spec may be one spec or a list (it holds when one does). A spec has `package` and/or `module` (a string or a list; at least one: a name alone would be a guess), optionally `name` (a string or a list) and `unknown: match | no-match` (default `no-match`).

When a rule loads, Forge rejects: `where` on a mode other than `forbid` or `advise` (`invalid rule: a where target's mode is forbid or advise`); a spec with neither `package` nor `module`; `deprecated` written other than `deprecated: true`; `deprecated` in a Python rule (`invalid rule: where deprecated is answered for TypeScript only (§15.7); Python's deprecations are ty's deprecated rule`); `needs` without `where` (`invalid rule: needs requires where`).

**Semantics.** For each match, each condition is evaluated on the code its metavariable is bound to; the match stands only when all of them hold.

1. **The expression's type** is what the language's checker infers for exactly that expression, in the file as it is on disk. The engine shows the server, in memory only, the file with the expression bound to a fresh name (Python `(__forge_tN := (<expr>))`, TypeScript `((__forge_tN = (<expr>)) => __forge_tN)()`), asks `textDocument/typeDefinition` at that name, then shows it the file as it is. Any expression works: a name, an attribute, a call such as `sessionmaker(e)()`, a subscript. Nothing is written to the project.
2. **Declarations.** Each location returned is the declaration of the type or of one member of a union or intersection. Import aliases, re-exports and type aliases are resolved by the checker: a declaration has the name it is declared with (`Session`, never the alias `Db`). An object type literal is named by the type alias that declares it. When TypeScript's servers give no type definition (an intersection, an alias of one), the engine reads the inlay hint of the bound name's type, the type as the checker prints it with each name linked to its declaration, and takes the names at its top level: the type it names, or each member of a union or intersection; never a type argument, an object or function type, or an array.
3. **Supertypes.** Each declaration counts with its supertypes, transitively, at most 64: Python through the server's type hierarchy; TypeScript through the `extends` and `implements` clauses of classes and interfaces, and the types a type alias's body is made of. A project class deriving from `Session` is a `Session`. Assignability is nominal: a compatible type that does not derive from the declaration does not count.
4. **Generics.** Type arguments are ignored: `list[Session]` is `list`.
5. **Where a declaration is** decides its package and module. Python: under `site-packages` or `dist-packages`, the package is the first path segment (`<pkg>-stubs` counts as `<pkg>`) and the module the dotted path without `__init__`; typeshed's `stdlib/` and `stubs/<dist>/` likewise; the standard library's own sources (`lib/python3.X/`) too. TypeScript: under the innermost `node_modules/`, the package is `<name>` or `@scope/name` (`@types/x` counts as `x`) and the module `<package>/<path without extension>`; TypeScript's own `lib.*.d.ts` are package `typescript`, module `typescript/lib/<name>`. A declaration in the project has no package; its module is its path (Python: dotted, without a leading `src.`; TypeScript: without the extension).
6. **A spec holds** when one declaration or supertype has the package (one of the list), the module (equal, or a prefix by whole segments: `.` in Python, `/` in TypeScript) and the name (one of the list) the spec gives. With a union, one member is enough.
7. **Unknown types.** When the checker does not know the type (Python `Any` or `Unknown`; TypeScript `any`, `unknown`, an error: no declaration), a spec does not hold unless it says `unknown: match`. Type conditions therefore rely on typed code, which the typing rules enforce (`python/no-unknown`, `python/no-explicit-any`, `typescript/no-implicit-any`, `typescript/no-explicit-any`).
8. `not` inverts its condition. A metavariable the match did not bind is not evaluated.
9. **Code that is not an expression.** A metavariable bound to code that cannot be bound as an expression (a parameter's or a keyword argument's name, a type) makes the server report a syntax error in the code the binding inserts. Its type cannot be asked: the match is dropped and the person gets the note `not checked: <file>:<line>: $<VAR> is bound to `<code>`, which is not an expression: its type cannot be asked` for the rule. The file's other matches are still answered.

**Stubs.** A library whose types the checker sees only through a stub package (`django-stubs`, `celery-types`) is named in the target's `needs: [{ecosystem, name}]`. Where the rule applies and the project does not declare that package, `forge status` reports the target `not enforced here: the types engine needs <package> installed to see the types this rule checks (add it as a development dependency)`.

**Where it runs.** The checker (§10) runs a rule's ast-grep target, keeps the matches it would report, and asks the engine about those whose rule has `where` (one `types` request per language, all matches at once). It keeps the matches the engine confirms (`holds`) and drops the others (`fails`, `unchecked`). After an edit it asks only an engine already running (100 ms to answer a ping, 300 ms for the question); `forge check` and Stop start it when none runs and wait up to 60 s for the answer.

### 15.2 Engines

| Language | Server | Version | Environment |
|---|---|---|---|
| Python | `ty server` | Forge's pinned ty (§17) | the project's virtual environment: `$VIRTUAL_ENV` when it holds `bin/python`, else `.venv` |
| TypeScript | the project's own TypeScript (`node_modules/typescript`, resolved from the root upwards): 7 and later, its language server (`tsc --lsp --stdio`, the native compiler `@typescript/typescript-<platform>-<arch>/lib/tsc`, else `bin/tsc` on a JavaScript runtime); 5 and 6, its `lib/tsserver.js` on a JavaScript runtime | the project's | the project's `tsconfig.json` files and `node_modules` |

- The JavaScript runtime is `node` on `PATH`, else `bun`. TypeScript 5 or 6 without one does not start: `TypeScript <version> needs a JavaScript runtime for tsserver, and none is configured`.
- A Python environment, when found, is given to `ty server` as its active environment (`pythonExtension.activeEnvironment`: the interpreter and its prefix), so ty never takes the Python first on `PATH`.
- A project without `node_modules/typescript` at its root gets the note `the TypeScript types engine needs the project's TypeScript installed (node_modules/typescript); the rules that ask for types are not checked`. A missing ty gets `the Python types engine needs ty: <why>; the rules that ask for types are not checked`.
- **Installed packages without typing are Any.** ty infers the types of an installed package that ships no typing (PEP 561: no `py.typed`, no `.pyi`, no `<name>-stubs` installed) from its source, which rejects correct code on libraries with runtime magic. When ty is one executable and the environment is found, Forge lists the environment's untyped modules (a package, a module, or a namespace package by portion such as `google.cloud`; the first site directory holding a name wins) and leaves out the modules ty resolves without the environment (the standard library stubs ty bundles, the project's own code: asked from the pinned ty with a probe file and an empty environment). The questions' server gets that list, less the packages the active rules' `where` conditions name, as `analysis.replace-imports-with-any` (`<name>` and `<name>.**`) in its inline configuration; a rule must see the declarations of the package it asks about. The edit-time diagnostics (15.5) come from a second `ty server` that gets the whole list, as `ty check` would. Nothing is written into the project's configuration. A list that cannot be computed is a note: `the packages without typing could not be listed (<why>); ty checks them as it infers them`.
- `forge.yaml` `python: {untyped-packages: any | source}` is accepted, but no code reads it yet: the list above is always applied to the engine, and never to `ty check`.

### 15.3 The daemon

- **One daemon per repository**, `forge engine serve --config <file>`, on the Unix socket `<cache>/engine/<repo-id>.sock`. A socket path longer than 103 bytes is replaced by `$TMPDIR/forge-<16 hex digits>.sock` (or `/tmp/…`). Its configuration is `<state>/<repo-id>/engine.json`, written atomically by the process that starts it; it logs to `<state>/<repo-id>/engine.log`. The servers of the configured languages start at once, so the first question finds them ready.
- **Start.** `forge check` and Stop start it when no daemon answers; SessionStart starts `forge engine start --root <root> --session <id>` in a detached process (its own process group, no stdio) when an active rule type-checks or asks types, `FORGE_ENGINE` is not `off`, and no start is under way; that process starts the daemon only when an active rule has a `where` condition, and registers the session. A project whose only typed rule is a typecheck therefore has no daemon, and its edit-time typecheck (15.5) is silent. Starters are serialised by a lock directory `<socket>.lock` holding the starter's pid; a lock whose pid is dead is taken over, and a starter that gets the lock first checks whether another one started it meanwhile. A start waits at most 60 s. Each configuration has a `want`: a hash of Forge's version and the engine configuration (the ty command, the environment, the untyped lists, the TypeScript settings). A daemon answering with another `want` is stopped and a new one started. A second daemon on a socket where one answers refuses to start.
- **Protocol.** One JSON object per line each way; a connection may carry several requests. Requests are `{"op": …}`:

| `op` | Fields | Answer |
|---|---|---|
| `ping` | | `status`: pid, root, want, the running servers (`ty 0.0.86`, `tsc 7.0.2`, `tsserver 5.9.3`), the languages whose server failed and why, the languages still loading, the number of sessions |
| `register` / `unregister` | `session` | `status` / `stopping`: true when the last session left |
| `changed` | `paths` (repository-relative) | nothing; a dependency or configuration file restarts the servers (below) |
| `types` | `lang`, `matches` (file, rule, line, bindings with byte offsets and text, `where`) | `verdicts`: per match `holds`, `fails` or `unchecked` with `why` |
| `diagnostics` | `lang`, `files` (path and base copy), `warnings` | `diagnostics`: those the change added (15.5) |
| `restart` | | the servers stop; the next question starts them |
| `stop` | | answered once the servers have exited |

  An unreadable line gets `{"error": "unreadable request: …"}`. Pings, registrations and notifications are answered at once; questions one at a time, in order. A failed answer is `{"error": "<server> <why>"}`. A server that failed to start is tried again after 30 s; one that died is started again by the next question.
- **What the server sees.** The file a question names is read from disk and sent when it differs from what the server has. UserPromptSubmit sends `changed` with the uncommitted files and those changed between the previous turn's HEAD and the current one. A changed `uv.lock`, `poetry.lock`, `pdm.lock`, `Pipfile.lock`, `pyproject.toml`, `ty.toml`, `package.json`, `bun.lock`, `bun.lockb`, `package-lock.json`, `pnpm-lock.yaml`, `yarn.lock` or `tsconfig*.json`, in any directory, restarts the servers instead. Nothing is written in the repository.
- **Loading.** TypeScript loads the project's tsconfig projects in the background once started (one tracked source file under each `tsconfig.json`), questions first. A question that times out while its language is loading is `Loading`: after an edit it is silent (the Stop check asks again); at Stop and in `forge check` it is a note.
- **End.** SessionEnd unregisters its session; the last session to leave stops the daemon. It also exits after 1800 s without a request, and within 10 s of its repository directory disappearing or its socket being replaced by another daemon's.

### 15.4 File roles

Rules name the files they are about by **role**: `scope.files: ["@python-tests"]`, `"!@alembic-migrations"`, `when_changed: ["@compose"]`, `forge_role @<role> <path>` in a check command, `module: "@wrangler-types"` in a where spec. Each role has Forge's defaults; the tool's own configuration and `forge.yaml` change them:

```yaml
files:
  python-tests: {add: ["integration/**/check_*.py"]}   # added to the defaults or the tool's config
  env-files: {set: [".env", ".env.local"]}             # replaces the defaults and the tool's config
```

- **Effective patterns** are computed when rules are selected (`forge apply`, `forge check`, the hooks' cached selection). `forge status` prints a `Files:` section: each role an active rule names, its patterns with where each comes from (`forge default`, the config file, `forge.yaml`), `; matches no file here` for a role no file matches, and what the tool's config said that Forge could not use (`--json`: `files`). The lock records each role's patterns (`[roles]`, §12); a changed role is drift: `files @<role> changed since the last apply: <patterns>`.
- **In rules.** `@<role>` in a glob list stands for the role's patterns, its exclusions included; `!@<role>` excludes its files, and a role with exclusions cannot be negated. A role a rule names that Forge does not know rejects the rule when it loads (`invalid rule: unknown file role @<name> (§15.4)`). A rule whose file selection is only roles without a pattern is not enforced and never runs on other files. A where `module` naming a role stands for the modules of the role's files (Python: dotted, without `src.`; TypeScript: the path without its extension); such a role must list files, not globs. In a check command a role is an unquoted `@<role>` word; quoted text (`grep -q '@apply'`) is the command's own.
- **Defaults from the tool's config** are read only from data files at the repository root, never from code, and replace Forge's defaults for that role. What a config says that is not a repository path (an Alembic package resource `pkg:dir`, a Flyway `classpath:` location) is kept as a note shown under the role, and the defaults stay. Code configs (`vitest.config.*`, `playwright.config.*`, `drizzle.config.*`, `prisma.config.ts`) are never evaluated.
- `forge.yaml` naming an unknown role, and a cycle between roles, are errors.
- Patterns are globs: `**` spans directories, `*` and `?` one segment, `[0-9]` and `[!x]` one character, `{a,b}` alternatives.

| Role | Defaults | From the tool's config |
|---|---|---|
| `adr` | `docs/adr/**`, `doc/adr/**`, `docs/decisions/**` | adr-tools `.adr-dir` + `/**` |
| `alembic-migrations` | `alembic/versions/**`, `migrations/versions/**` | `alembic.ini` `[alembic]` (else `pyproject.toml` `[tool.alembic]`): `version_locations` + `/**` (split as `path_separator` says), else `script_location` + `/versions/**` |
| `angular-tsconfig` | `**/tsconfig.app.json` | `angular.json`: each project's `architect` (or `targets`) `build.options.tsConfig` |
| `better-auth-config` | the auth CLI's search paths: `{,src/,app/}{,lib/server/,server/auth/,server/,auth/,lib/,utils/}{auth,auth/index}{.ts,.tsx,.js,.jsx,.server.js,.server.ts}` | — |
| `build-artifacts` | `**/node_modules/**`, `**/__pycache__/**`, `**/*.pyc`, `**/.next/**`, `**/.nuxt/**`, `**/.svelte-kit/**`, `**/.turbo/**`, `**/.coverage`, `**/.DS_Store` | — |
| `compose` | `**/compose*.yaml`, `**/compose*.yml`, `**/docker-compose*.yaml`, `**/docker-compose*.yml` | — |
| `django-migrations` | `**/migrations/[0-9]*.py` | — |
| `django-settings` | `**/settings.py`, `**/settings/*.py` | pytest-django `DJANGO_SETTINGS_MODULE` in the pytest options (`a.b` → `a/b.py`, `a/b/*.py`) |
| `django-templates` | `**/templates/**/*.html`, `**/templates/**/*.txt` | — |
| `dockerfile` | `**/Dockerfile`, `**/Dockerfile.*`, `**/*.Dockerfile`, `**/Containerfile`, `!**/*.dockerignore` | — |
| `docs` | `docs/**/*.{md,mdx,rst}`, `doc/**/*.{md,mdx,rst}`, `README*` | — |
| `drizzle-migrations` | `drizzle/**` | — (code config) |
| `e2e-tests` | `e2e/**`, `tests/e2e/**` | — (code config) |
| `env-files` | `**/.env`, `**/.env.*`, `!@env-template` | — |
| `env-template` | `**/.env.example`, `**/.env.sample`, `**/.env.template`, `**/.env.dist` | — |
| `esm-modules` | `**/*.mts`, `**/*.mjs` | plus `<dir>/**/*.{ts,tsx,js,jsx}` for each `package.json` with `"type": "module"`, less those under a nested `package.json` without it |
| `generated-clients` | none | each `generator` block's `output` in the Prisma schema (relative to it) + `/**` |
| `gunicorn-config` | `**/gunicorn.conf.py` | — |
| `lockfiles` | `uv.lock`, `bun.lock`, `bun.lockb`, `package-lock.json`, `pnpm-lock.yaml`, `yarn.lock` | — |
| `prisma-migrations` | `prisma/migrations/**` | `package.json` `prisma.schema` (its directory) + `/migrations/**` |
| `procfile` | `Procfile`, `Procfile.*` | — |
| `python-tests` | `**/test_*.py`, `**/*_test.py`, `**/conftest.py` | pytest `testpaths` × `python_files` (default `test_*.py`, `*_test.py`), plus `**/conftest.py`; the options come from the first of `pytest.ini` `[pytest]`, `pyproject.toml` `[tool.pytest]` / `[tool.pytest.ini_options]`, `tox.ini` `[pytest]`, `setup.cfg` `[tool:pytest]` |
| `sql-migrations` | `db/migration/**/*.sql` (Flyway), `supabase/migrations/*.sql`, `db/migrations/*.sql` | Flyway `flyway.locations` `filesystem:` directories + `/**/*.sql` (`flyway.conf`, `conf/flyway.conf`, else `flyway.toml`) replace Flyway's default; with `sqitch.plan`, `deploy/**/*.sql` under `sqitch.conf` `core.top_dir`; with `atlas.hcl`, each `dir = "file://…"` + `/*.sql` (default `migrations/*.sql`) |
| `ts-tests` | `**/*.{test,spec}.{ts,tsx,mts,cts,js,jsx,mjs,cjs}`, `**/__tests__/**` | — (code config) |
| `typeorm-datasource` | none: set it in `forge.yaml` | — (code config) |
| `wrangler-config` | `**/wrangler.toml`, `**/wrangler.json`, `**/wrangler.jsonc` | — |
| `wrangler-types` | `worker-configuration.d.ts` | — (a CLI argument) |

### 15.5 The edit-time typecheck

The asyncRewake PostToolUse hook (§8) asks a running daemon for the `diagnostics` of the edited files of each type-checked language, each with its base copy (the file at the start of the turn, else at HEAD; none for a new file). A diagnostic counts when it is an error (a warning too with `warnings`) and the file now has more of it than its base, compared without the location, so moved code keeps its errors. The hook then exits 2 with `This edit added type errors:` and one `<path>:<line>:<column>: <severity>[<code>] <message>` line per diagnostic, at most 20, then `(<n> more)`. No engine running, no error added, or an engine still loading: it is silent and the Stop check covers the typecheck.

### 15.6 Declared names

```yaml
# packs/python/numpy/rules/no-allow-pickle.yaml
    where:
      F: {declared: {package: numpy, name: load}}
```

`{declared: <spec>}` holds when the name or attribute the metavariable is bound to **refers to** a declaration the spec describes (15.1 rules 5 and 6, the same fields): for functions, decorators, classes and modules used by name, where `type` would describe the value.

1. The expression must end with a name: a name (`load`) or an attribute (`np.load`); its last name is asked. Any other expression (a call, a subscript) is unknown.
2. The declaration is what the checker answers to `textDocument/definition` at that name, in the file as it is on disk. The checker resolves import aliases, namespace imports and re-exports, the project's included. Overloads give one declaration each. No supertypes are added.
3. A module (`import numpy as np`, `$F` bound to `np`) is declared by its file, its name the module's last segment (Python).
4. **Unknown.** A name with no definition (an attribute of `Any`), one whose definition stops at an import the checker could not resolve, and an expression that does not end with a name are unknown: the spec does not hold unless it says `unknown: match`.
5. A name bound to a project variable refers to that variable (`run = np.load`, then `run(…)`): the declaration is the project's, whatever its value.

A rule may mix `type` and `declared` conditions on different metavariables.

### 15.7 Deprecated uses

```yaml
# packs/typescript/rules/no-deprecated.yaml
    where:
      N: {deprecated: true}
```

`{deprecated: true}` holds when the name the metavariable is bound to is a use of a declaration marked `@deprecated`, as the project's TypeScript reports it:

1. **Uses the checker reports**: suggestion diagnostics `6385` and `6387` (a reference, a property access, the overload a call resolves to, a type, an import). The name holds when its range overlaps one. tsserver answers `suggestionDiagnosticsSync`; `tsc --lsp` answers `textDocument/diagnostic`.
2. **Object literal keys**, which TypeScript checks but never reports: a name in a key's position holds when the server documents the property as deprecated (tsserver's `quickinfo` kind modifiers; the language server's hover, which renders the tag as `*@deprecated*`) and its definition is not the name itself.
3. A declaration is never a use.

Only TypeScript's engine answers it; Python's deprecations are ty's own `deprecated` rule (`python/no-deprecated`).

### Tests

- 15.1 schema and failure rules: `crates/forge-engine/tests/spec_engine.rs` (`where_conditions_are_checked_when_rules_load`, `judge_drops_what_it_cannot_confirm_and_says_so`, `a_project_without_its_typescript_is_unchecked_never_passed`), `crates/forge-engine/src/spec.rs` (unit: `where` parses; module prefixes by whole segments; `unknown: match`), `crates/forge-engine/src/lib.rs` (unit: failures are notes). Semantics on real servers: `crates/forge-engine/tests/engine.rs` (`python_engine` with ty and SQLAlchemy: aliases, `sessionmaker()`, subclasses, `Any`, `not`, a keyword name unchecked; `typescript_5_engine`, `typescript_6_engine`, `typescript_7_engine`: intersections, `Map` look-alikes, declared names, deprecated uses), each skipped with a printed reason when uv or bun is missing; `crates/forge-engine/src/python.rs`, `tsscan.rs` (unit: bindings).
- 15.2: `crates/forge-engine/tests/engine.rs` (the server per TypeScript major). (no test: the untyped-packages list, which needs a real environment and ty; `crates/forge-helpers/src/untyped.rs` has a unit test of a site's modules.)
- 15.3: `crates/forge-engine/tests/daemon.rs` (a server that cannot start is a note; sessions and idle exit; another `want` replaces the daemon), `crates/forge-engine/tests/spec_engine.rs` (`a_dependency_file_change_restarts_the_servers_and_a_source_change_does_not`, `one_daemon_per_socket`, `json_lines_several_requests_per_connection`, `the_daemon_exits_when_its_repository_disappears`), `crates/forge-engine/src/lib.rs` (unit: long socket paths).
- 15.4: `crates/forge-detect/tests/spec_roles.rs` (every config reader, `forge.yaml` add and set, unknown roles and cycles, roles in rules, empty roles, quoted `@` words, where modules, unknown roles at load), `crates/forge-detect/src/roles.rs` (unit), `crates/forge-packs/src/roles.rs` (unit). (no test: the `Files:` section of `forge status` and role drift in `forge diff`.)
- 15.5: `crates/forge-engine/tests/engine.rs` (`python_engine`: only the error the edit added), `crates/forge-engine/src/diag.rs` (unit). The hook's output: §8's tests.
- 15.6, 15.7: `crates/forge-engine/tests/engine.rs` (declared and deprecated cases per language).
