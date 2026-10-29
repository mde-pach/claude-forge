## 5. Inputs

### 5.1 Packs and the loader

Packs form a tree: **language first, then package, then sub-package** (`typescript` → `typescript/nextjs` → `typescript/nextjs/react`). The same package can appear under several parents with different rules (`typescript/nextjs/react`, `typescript/astro/react`). Packs not tied to a language are roots of their own (`safety`, `code`, `docs`, `secrets`, `git`, …). §13 lists every built-in pack.

- The built-in packs are the repository's `packs/` directory, embedded in the binary at build time (`include_dir!`) and parsed at run time. The binary's version therefore pins the built-in set.
- Local packs (`.forge/packs/`) and local rules (`.forge/rules/`) go through the same loader (§5.6).
- The YAML reader is strict: anchors, aliases, custom tags and duplicate keys are rejected. Scalars follow YAML's core schema.
- Each file is validated against its JSON Schema (`schemas/pack.schema.json`, `rule.schema.json`, `project.schema.json`, generated from the Rust model), then by checks a schema cannot express (§6.5). A file that fails is an error that stops the command, naming the file.
- The content lints (§6.5) do not stop loading. `forge packs lint` reports them; the built-in packs must have none.
- A parsed tree is cached under `<cache>/packs/` (MessagePack), keyed by a hash of its inputs and of the build. Built-in and local trees are cached separately. A missing, stale or corrupt entry is reparsed and rewritten; the cache never changes what is loaded.

### 5.2 Pack directory and `pack.yaml`

```
packs/<path>/                   (or .forge/packs/<path>/)
├── pack.yaml
├── rules/<name>.yaml           rule id: <path>/<name>
├── init/                       optional: templates forge init creates (§3.2)
└── <child>/                    optional child packs (a directory with a pack.yaml)
```

`rules` and `init` are reserved names; any other sub-directory must be a child pack. A pack's `id` equals its path in the tree, and each rule's `id` equals `<pack id>/<file name>`. A `context.md` file is refused: a pack's context is its `pack.yaml` `context:` list.

```yaml
id: string                  # the pack's path in the tree: ^seg(/seg)*$, seg = ^[a-z][a-z0-9-]*$
description: string
level: user | project       # equal to the parent's level
requires: [pack-id]         # packs it needs, in other branches (the parent is implied)
detect:                     # project level only; any one of these detects the pack (§5.3)
  always: true
  files_any: [glob]
  deps_any: [{ecosystem: pypi | npm, name: string, version: range}]
  library_any: [pypi | npm]
  content_any: [{files: [glob], pattern: regex}]
  yaml_keys: [key]
  sources_any: [glob]
  facts_any: {<fact>: value | [value]}
tool_off:                   # optional: tool diagnostics the pack switches off where it is active
  - {tool: biome, expects: lint/<group>/<name>, reason: string}
context:                    # optional: what the agent reads while the pack is active (§5.4)
  - id: string              # optional; items with the same id are rendered once
    text: string
    when:                   # optional; every condition given must hold
      deps: [{ecosystem, name, version}]
      files: [glob]
      facts: {<fact>: value | [value]}
      packs: [pack-id]
```

- `requires` and every `when.packs` must name loaded packs; a local pack may require built-in packs.
- **`tool_off`.** Where the pack is active, no rule's `tool` target with that `tool` and `expects` is audited or demanded (nor its rule's `ast-grep` stand-in). `forge status` lists the rule as `not enforced here: <tool> <expects> switched off by <pack> (<reason>)` and under `Switched off by packs:` (`--json`: `switched_off`, with the rules that asked for it). `forge init` writes the diagnostic as `"off"` in Biome's config (`linter.rules.<group>.<name>`). It is for a diagnostic whose fix is harmful in that framework (NestJS: `useImportType`).

### 5.3 Detection

A root pack is active when it is detected; a child pack is active when its parent is active **and** it is detected. Any one detect entry is enough.

| Kind | Detected when |
|---|---|
| `always: true` | Always (a root pack not tied to a language). |
| `files_any` | One of the place's own files matches a glob. |
| `deps_any` | One of the dependencies is declared, in its version range. |
| `library_any` | The project publishes a library to that ecosystem (below). |
| `content_any` | A line of a file matching `files` matches `pattern` (a regular expression with `^`/`$` anchoring lines). A pattern that does not compile rejects the pack. |
| `yaml_keys` | A `.yaml`/`.yml` file's first document is a mapping with all these top-level keys. A file that is not YAML (a template that renders to YAML) does not count. |
| `sources_any` | A source file of the project's own matches a glob (below). |
| `facts_any` | A named fact (§5.4) has one of the values. |

- **What detection reads.** The file list is git's: tracked files and untracked files that are not ignored (`git ls-files --cached --others --exclude-standard`), so ignored directories never count. `content_any` and `yaml_keys` read file contents only for packs that use them, and only files of at most 1 MiB.
- **Dependencies.** npm: `package.json` `dependencies`, `devDependencies`, `optionalDependencies`, `peerDependencies`, and the package manager it pins (`packageManager`, else `devEngines.packageManager`) as a dependency at that version. PyPI: `pyproject.toml` `[project] dependencies`, `optional-dependencies`, `[dependency-groups]`, `[tool.uv] dev-dependencies`, Poetry's dependency tables; `requirements*.txt` at the top level and `requirements/*.txt`; `Pipfile`. Versions are resolved from the root's lock files (npm: `bun.lock`, `pnpm-lock.yaml`, `yarn.lock`, `package-lock.json`; PyPI: `uv.lock`, `poetry.lock`, `pdm.lock`, `Pipfile.lock`), else the lowest declared version. Ranges use npm semver syntax; a version that cannot be read (`workspace:*`, a URL) satisfies every range.
- **Libraries.** A Python library is a `[project]` with a `name` and a `[build-system]`, no `scripts` or `gui-scripts`, no `manage.py`, no `Private ::` classifier, and evidence of publishing (classifiers, or CI that uploads to PyPI). An npm library is a named, non-private package that exports an API (`exports`, or `main`/`module` with `types`/`typings`/`files`) and is published (`publishConfig`, or CI that publishes to npm). CI evidence is read from `.github/workflows/*.y*ml` and `.gitlab-ci.yml` (`pypa/gh-action-pypi-publish`, `uv|poetry|hatch|flit|pdm publish`, `twine upload`; `npm|pnpm|bun publish`, `yarn [npm] publish`, `changesets/action`, `semantic-release`, `JS-DevTools/npm-publish`) and from `.changeset/config.json`.
- **Sources.** A file counts for `sources_any` when no directory on its path is `node_modules`, `.venv`, `venv`, `.env`, `env`, `virtualenv`, `site-packages`, `dist-packages`, `vendor`, `vendored`, `_vendor`, `third_party`, `third-party`, `build`, `dist`, `.tox`, `.nox`, `.eggs`, `__pycache__`, `.mypy_cache`, `.git` or `*.egg-info`, it is not under `.forge/` or `.claude/`, and it is not a generated protobuf module (`*_pb2.py`, `*_pb2_grpc.py`). It detects a project by its own source files when it has no packaging file (plain scripts).
- **`forge.yaml`.** `packs.add` activates a pack and its ancestors; `requires` activates the required pack and its ancestors; `packs.remove` deactivates a pack, its whole subtree, and every pack that requires a removed pack. An unknown pack, or a user-level pack in `packs.add`, is a warning.
- **Workspaces.** The workspace packages of a monorepo are the directories holding a `package.json` or `pyproject.toml` matched by `pnpm-workspace.yaml` `packages`, `package.json` `workspaces` (array, or `{packages}`), `[tool.uv.workspace] members`, or `lerna.json` `packages` (default `packages/*`), minus `!` patterns and uv's `exclude`. Without a declaration there are none. Directories under `node_modules` or with characters outside `[A-Za-z0-9._@+/-]` are not read. The root and each package are judged on their own dependencies, files and facts; versions are never merged across packages. A pack is detected when it is detected at the root or in one package; `files_any` globs match relative to each package, and the root's files are those outside every package.
- **Where rules apply.** A pack detected only in some workspace packages (not at the root, not added in `forge.yaml`, not required) applies to those packages: its ast-grep and grep rules are scoped to them, and its rules file gets `paths:` frontmatter (§7.1). Its guard, permission and sandbox entries apply to the whole repository. A check runs at the root when its pack is detected there and the root configures the audited tools it needs, else in each package that has its pack and configures them; `forge status` shows `(in <dirs>)`.

### 5.4 Facts and context items

Facts are named values read from the repository's files, the same on every machine. Each place (the root, each workspace package) has its own.

**`python.pm`**, first match wins:

| Files | Fact |
|---|---|
| `uv.lock` (here or at the root) | `uv` |
| `poetry.lock` (here or at the root) | `poetry` |
| `pdm.lock` (here or at the root) | `pdm` |
| `Pipfile.lock` or `Pipfile` | `pipenv` |
| `pyproject.toml` whose `[tool.poetry]` declares dependencies | `poetry` |
| `pyproject.toml` with a `[project]` table or `[dependency-groups]`, no lock | `uv` |
| `requirements*.txt` or `requirements/*.txt` (no pyproject, or one holding only tool config) | `pip` |
| `setup.py` or `setup.cfg` | `pip` |
| none of these (plain scripts, or no Python) | `none` |

**`js.pm`**: the package manager pinned by `packageManager` or `devEngines.packageManager` (the package's, then the root's); else the lock file (`bun.lock`/`bun.lockb` → `bun`, `pnpm-lock.yaml` → `pnpm`, `yarn.lock` → `yarn`, `package-lock.json`/`npm-shrinkwrap.json` → `npm`; the package's directory, then the root); else `npm` when there is a `package.json`; else `none`.

**Context items.** Each active pack's `context` items are rendered into its rules file (§7.1), in pack-id order:

- An item without `when` is rendered wherever its pack is active, so its text must hold for every project the pack is detected in.
- An item with `when` is rendered when one of its `when.packs` is active (if given) and, in one place the pack applies to, every other condition holds: each `deps` entry declared in range, a file matching one of `files`, each `facts` entry equal to one of its values. A pack applies to the root and the packages it was detected in; a pack detected nowhere (added, required) is judged at the root alone.
- An item whose `id` an earlier pack already rendered is left out: shared norms are written once (the `code` pack holds the norms the language packs share).
- At user level there are no facts: only items without conditions, or with `packs` alone, can hold.
- Context states norms, as facts about the project. It never claims that a tool enforces them (the `enforcement-claim` lint, §6.5), never names Forge's internals, and never enforces anything: enforcement is done by checks and hooks.

### 5.5 `forge.yaml`

```yaml
forge: "0.5"                # required: ^\d+\.\d+$
packs:
  add: [pack-id]
  remove: [pack-id]
disable:
  - id: <rule-id>
    reason: string          # required
accept:                     # config-ratchet findings accepted on purpose (§10)
  - rule: <rule-id>
    change: string          # the finding line exactly as forge check printed it
    reason: string          # required
files:                      # file roles (§15.4)
  <role>: {add: [glob], set: [glob]}
python:
  untyped-packages: any | source   # installed packages without typing: Any to ty (default), or inferred from their source (§15.2)
```

- A `disable` naming a rule that is not active is a warning.
- `forge.yaml` is an input: `apply` never writes it. It is behind a permission `ask` (§7.2).

### 5.6 Local packs and rules

- `.forge/packs/<id>/` holds local packs, laid out as built-in packs are (§5.2). A local pack's id may not be a built-in pack's id: `pack id "<id>" is a built-in pack's; a local pack needs an id of its own`.
- `.forge/rules/<name>.yaml` holds standalone rules. A local rule whose `id` equals an active pack rule's `id` replaces it entirely. A local rule is kept when its pack is active or is not a known pack at all; one for a known pack that is not active is ignored, with the warning `<file>: pack "<pack>" is not active here; rule <id> is ignored`.
- Local packs and rules are inputs, behind the permission `ask` on `Edit(.forge/**)` (§7.2). They are validated and linted as built-in ones are, by every command that loads packs.

### Tests

- Strict YAML, schemas, layout, local packs and rules, the cache: `crates/forge-packs/tests/loading.rs`, `crates/forge-packs/tests/schemas.rs`, `crates/forge-packs/src/yaml.rs` (unit).
- Lints: `crates/forge-packs/tests/lints.rs`, `crates/forge-packs/src/lint.rs` (unit).
- Detection kinds, dependencies, workspaces, libraries, vendored directories: `crates/forge-detect/src/*.rs` (unit); the generator samples in `crates/forge-cli/tests/apply.rs`.
- `python.pm` and `js.pm`: `crates/forge-detect/src/pm.rs::python_pm_table`, `::js_pm_cases` (unit).
- Context items and `when`: `crates/forge-render/tests/context.rs`, `crates/forge-render/tests/builtin_context.rs`.
