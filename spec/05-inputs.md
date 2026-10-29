## 5. Inputs

### 5.1 Packs

Packs form a tree: **language first, then package, then sub-package** (`typescript` → `typescript/nextjs` → `typescript/nextjs/react`). The same package can appear under several parents with different rules (`typescript/nextjs/react`, `typescript/astro/react`). Packs that are not tied to a language are roots of their own: `safety` (user level), `docs`, `secrets`, `git` (project level).

`packs/<path>/pack.yaml`:

```yaml
id: string                  # the pack's path in the tree: ^seg(/seg)*$, seg = ^[a-z][a-z0-9-]*$
description: string
level: user | project       # equal to the parent's level
requires: [pack-id]         # other branches only; the parent is implied
detect:                     # omitted for user-level packs; any one of these detects the pack
  always: true               # every repository (a root pack not tied to a language: secrets, git)
  files_any: [glob]
  deps_any: [{ecosystem: pypi | npm, name: string, version: semver-range}]
  library_any: [pypi | npm]  # the project is a library published there (§13.4)
  content_any: [{files: [glob], pattern: regex}]  # a line of a matching file matches (a literal value in a literal config file)
  yaml_keys: [key]           # a .yaml/.yml file whose first document is a mapping with all these top-level keys
  sources_any: [glob]        # a source file of the project's own: outside vendored, generated and virtual-environment directories
tool_off:                   # optional: tool diagnostics this pack switches off where it is active
  - {tool: biome, expects: lint/<group>/<name>, reason: string}
```

- A root pack is active when detected; a child pack is active when its parent is active **and** it is detected.
- `content_any` and `yaml_keys` read file contents, only for packs that use them and only files of at most 1 MiB. `content_any` patterns are JavaScript regular expressions tested on each line (`^` and `$` anchor lines); one that does not compile rejects the pack. `yaml_keys` reads a file's first document (up to the next `---` or `...`); a file that is not YAML (a template that renders to YAML) does not count. Detection by content stays a detection: `packs.add` and `packs.remove` decide last.
- `sources_any` detects a project by its own source files when it has no packaging file (plain scripts). A file counts when it matches a glob and no directory on its path is `node_modules`, `.venv`, `venv`, `.env`, `env`, `virtualenv`, `site-packages`, `dist-packages`, `vendor`, `vendored`, `_vendor`, `third_party`, `third-party`, `build`, `dist`, `.tox`, `.nox`, `.eggs`, `__pycache__`, `.mypy_cache`, `.git` or `*.egg-info`, it is not under Forge's `.forge/` or `.claude/` (Forge's own helper scripts), and it is not a generated module (protobuf's `*_pb2.py`, `*_pb2_grpc.py`). One such file is enough. The file list is git's (tracked and untracked not ignored), so ignored directories never count. Packs whose facts come from packaging (a lock file, declared dependencies, a published library) keep their own detection, so a pack detected by `sources_any` activates none of them.
- **`tool_off`.** Where the pack is active, no rule's `tool` target with that `tool` and `expects` is audited or demanded (nor its rule's `ast-grep` stand-in): status lists it as `not enforced here: <tool> <expects> switched off by <pack> (<reason>)` and under `Switched off by packs:` (`--json`: `switched_off`, with the rules whose targets asked for it), and `forge init` writes Forge's config for that tool with the diagnostic `"off"` (Biome: `linter.rules.<group>.<name>` in `biome.json`, the rest of the template kept). For a diagnostic the tool's own fix makes harmful in that framework (NestJS: `useImportType`).
- `forge.yaml` `packs.add` activates a pack and its ancestors. `requires` activates the required pack and its ancestors.
- `packs.remove` deactivates a pack, its whole subtree, and every pack that requires a removed pack.
- Detection reads `pyproject.toml`, `uv.lock`, `package.json`, `bun.lock`, and the files `content_any` and `yaml_keys` name.
- **Workspaces.** Detection also reads the workspace packages of a monorepo: the directories (holding a `package.json` or `pyproject.toml`) matched by `pnpm-workspace.yaml` `packages`, `package.json` `workspaces` (array, or `{packages}`), `[tool.uv.workspace] members`, or `lerna.json` `packages` (default `packages/*`), minus `!` patterns and uv `exclude` (an exclusion like `**/test/**` also excludes the `test` directory itself); without any declaration, the repository has no workspace packages. Globs support `**`, `*`, `?` and `{a,b}` alternatives, which may nest and hold `**`. The file list is read once and bucketed by package. A pack is detected when it is detected at the root or in one package, each judged on its own dependencies and files (versions are never merged across packages; `files_any` globs match relative to each package, and the root's files are those outside the packages). The root's lock files resolve versions.
- **Where checks run.** A `check` target that runs a project tool (and no engine helper other than `forge_exec_tool`, which runs a project tool whose name has a `-`, e.g. `vue-tsc`) runs at the root when its pack is detected at the root and the root configures its audited tools (as without workspaces). Otherwise it runs in each workspace package that has its pack (any package, for a pack active at the root) and configures those tools (a config file, or a dependency when no config file is at the root); `status` shows `(in <dirs>)`. There, `{files}` gets the package's changed files relative to it, the tool is the package's own or the root's (hoisted), and relative paths in the output are prefixed with the package directory, so diagnostics are attributed as at the root. Its `format_stdin` filter runs in the package holding the file, on the path relative to it. Tool audits run in every package that configures the tool (`(in <dir>)` in the audit line), and a rule counts as enforced by the tool only when every package's config enforces it.
- **Where other rules apply.** A pack detected only in some workspace packages (not at the root, not added in `forge.yaml` or required) scopes its ast-grep rule files (`files` / `ignores`) and grep rules to those packages, its globs relative to each. Its guard, permission and sandbox entries and its context file apply to the whole repository, and helper checks (ratchet, migrations, Docker, test integrity) run from the root with globs relative to it. Package directories with spaces are not read.

### 5.2 Pack directory

```
packs/<path>/
├── pack.yaml
├── rules/<name>.yaml     # rule id: <path>/<name>
├── context.md            # optional, ≤ 10 lines of facts
├── init/                 # optional, templates used only by forge init
└── <child>/              # optional child packs (any directory with a pack.yaml)
```

`rules` and `init` are reserved names; any other sub-directory must be a pack.

### 5.3 Project inputs

```yaml
# <repo>/forge.yaml (optional)
forge: "0.5"
packs:
  add: [pack-id]
  remove: [pack-id]
disable:
  - id: <rule-id>
    reason: string          # required
accept:                     # config-ratchet findings accepted on purpose (§10.4)
  - rule: <rule-id>
    change: string          # the finding line exactly as check.sh printed it
    reason: string          # required
files:                      # file roles (§15.4)
  <role>: {add: [glob], set: [glob]}   # add to the role, or replace its defaults and the tool's config
python:
  untyped-packages: any | source       # installed packages without typing: Any to ty (default), or inferred from their source (§15.2)
```

`<repo>/.forge/rules/<name>.yaml`: project-local rules. A local rule whose `id` equals a pack rule's `id` replaces it entirely. `forge.yaml` and `.forge/rules/` are inputs, not owned output: `apply` never rewrites them, but they are behind permission `ask` (§8.3).
