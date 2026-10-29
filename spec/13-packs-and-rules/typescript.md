### typescript

| Rule | Targets |
|---|---|
| `typescript/a11y` | `tool` biome `lint/a11y/useAltText`, `lint/a11y/useButtonType` in `.tsx`, `.jsx`, `.vue`, `.svelte`, `.astro` and `.html` files (snippet: `html.experimentalFullSupportEnabled` and the whole `a11y` group at error; examples in each file type, §11) |
| `typescript/format-lint` | `check` lint, on TypeScript and JavaScript files and the `.vue`, `.svelte`, `.astro` and `.html` files Biome also reads (their templates with `html.experimentalFullSupportEnabled`): `run: biome ci .`, `changed_run: biome check --write {files}`, `format_stdin: biome format --stdin-file-path={file}`; no `fix_run`: `changed_run` applies Biome's safe fixes itself (removing unused imports or variables is unsafe, so it is not done) |
| `typescript/typecheck` | `check` typecheck: `run: tsc --noEmit -p tsconfig.json --pretty false --incremental --tsBuildInfoFile "$(forge_state_file tsc.tsbuildinfo)"` (plain diagnostics; incremental, TypeScript ≥ 4.0, with the build info under the repository root's `.forge/state/`, as an absolute path, one file per workspace package (`<package dir, / as ->.tsc.tsbuildinfo`), so a project's own or composite build info is never written and a package never gets a `.forge/` of its own) |
| `typescript/no-implicit-any` | `tool` tsc: expects `TS7006` on an untyped parameter (`noImplicitAny`, part of `strict`) |
| `typescript/strict-null-checks` | `tool` tsc: expects `TS18047` on a possibly-null access (`strictNullChecks`) |
| `typescript/strict-function-types` | `tool` tsc: expects `TS2322` on a parameter-bivariant assignment (`strictFunctionTypes`) |
| `typescript/no-explicit-any` | as §6.3 |
| `typescript/no-ts-ignore` | `tool` biome `lint/suspicious/noTsIgnore`; `ast-grep` forbid comment `// @ts-ignore` or `/* @ts-ignore */` |
| `typescript/no-non-null-assertion` | `tool` biome `lint/style/noNonNullAssertion`; `ast-grep` forbid `non_null_expression` |
| `typescript/no-double-assertion` | `ast-grep` forbid: `$X as unknown as $T`, `$X as any as $T` |
| `typescript/no-ts-nocheck` | `ast-grep` forbid comment `@ts-nocheck` |
| `typescript/ts-expect-error-reason` | `ast-grep` forbid comment `@ts-expect-error` with nothing after it |
| `typescript/no-deprecated` | `when` typescript `>=5.0.0` (the project's TypeScript answers); `ast-grep` forbid, `where` `{deprecated: true}` (§15.7) on every identifier, property, type and shorthand name: a use of a declaration marked `@deprecated` (the project's, a library's, `@types/node`'s: `url.parse`, `new Buffer()`, React's `ElementRef`) as TypeScript reports it, including the overload a call resolves to, and an object literal key of a deprecated property |
| `typescript/no-ignore-deprecations` | `check` lint: no `"ignoreDeprecations"` in tracked `tsconfig*.json`, `when_changed: [tsconfig*.json]` |
| `typescript/no-focused-tests` | `tool` biome `lint/suspicious/noFocusedTests`; `ast-grep` forbid `it.only(…)`, `test.only(…)`, `describe.only(…)`, `test.describe.only(…)` in test files and `e2e/**` |
| `typescript/no-eval` | `tool` biome `lint/security/noGlobalEval`; `ast-grep` forbid `eval(…)`, `globalThis.eval(…)`, `window.eval(…)` |
| `typescript/unchecked-index` | `tool` tsc: expects `TS18048` on an unchecked array element (`noUncheckedIndexedAccess`; snippet adds it to `strict`) |
| `typescript/no-new-suppressions` | `grep` no-increase: `biome-ignore`, `eslint-disable`, `oxlint-disable` (every form), `deno-lint-ignore`, `prettier-ignore`, `@ts-expect-error`, and the coverage hints `istanbul ignore`, `c8 ignore`, `v8 ignore` (suppression budget; `@ts-ignore` and `@ts-nocheck` are forbidden outright) |
| `typescript/no-empty-block` | `tool` biome `lint/suspicious/noEmptyBlockStatements` |
| `typescript/no-swallowed-catch` | `ast-grep` forbid: a `catch` that neither throws nor uses its error; `.catch(() => {})`, `.catch(() => undefined)`, `.catch(() => null)` |
| `typescript/no-console` | `tool` biome `lint/suspicious/noConsole` (snippet allows `warn` and `error`) |
| `typescript/no-unused` | `tool` biome `lint/correctness/noUnusedImports`, `lint/correctness/noUnusedVariables` |
| `typescript/no-secrets` | `tool` biome `lint/security/noSecrets` (snippet: error, `entropyThreshold` 50, off for tests and generated files) |
| `typescript/no-unused-suppressions` | `check` lint: `biome lint` output has no `suppressions/unused` diagnostic (Biome reports it only as a warning, and no setting raises it) |
| `typescript/no-stub` | `ast-grep` forbid, not in `*.d.ts`: a `throw` whose message starts with "Not implemented", "unimplemented" or "TODO"; a named function or method (not a constructor) whose body is empty or only comments |
| `typescript/no-placeholder-comment` | `grep` forbid (case-insensitive): the Python list, after `//`, `/*` or `*` |
| `typescript/config-files` | `permission` ask: `Edit(**/biome.json)`, `Edit(**/biome.jsonc)`, `Edit(**/tsconfig*.json)`, `Edit(**/eslint.config.*)`, `Edit(**/.oxlintrc.json)`, `Edit(**/deno.json)`, `Edit(**/deno.jsonc)`, `Edit(**/.prettierrc*)`, `Edit(**/prettier.config.*)`, `Edit(**/pnpm-workspace.yaml)`, `Edit(**/.npmrc)`, `Edit(**/.yarnrc.yml)`, `Edit(**/bunfig.toml)`, `Edit(**/turbo.json)` (code configs are not ratcheted, §10.4) |
| `typescript/config-ratchet` | `check` lint: `forge_ratchet bun ratchet.mjs` (§10.4), `when_changed`: Biome, tsconfig and `package.json` files |
| `typescript/node-version-file` | `check` lint: `forge_helper bun node_version.mjs` (§13.4), when `.nvmrc`, `.node-version` or `package.json` changed: the root `.nvmrc` / `.node-version` is a version (an alias such as `lts/*` is reported) satisfying `package.json` `engines.node` and `devEngines.runtime` (`name: node`) ranges; a partial version (`22`) stands for the newest release of its line (what nvm, fnm and setup-node install) |
| `typescript/lockfile-updated` | `check` lint: `forge_lockfile_updated`: a `package.json` whose `dependencies`, `devDependencies` or `optionalDependencies` changed since the base needs its lock file (`pnpm-lock.yaml`, `package-lock.json`, `npm-shrinkwrap.json`, `yarn.lock`, `bun.lock`; next to it, else at the root) to have a changed line naming each changed dependency; `bun.lock` is skipped when `typescript/bun/lock-sync` is active (`forge_check_active`); blocks once per turn (`forge_once`) |
| `typescript/env-example` | `check` lint: `forge_env_example ts`, as `python/env-example` for the UPPER_CASE keys of a Zod env schema (t3-env `createEnv`, or `z.object(…)` with `process.env`) whose value is a `z.` chain; a key with `.default()`, `.optional()` or `.catch()` counts as having a default |
| `typescript/no-html-sinks` | `ast-grep` forbid, `where` the receiver is `typescript` `Element`, `Document`, `Range` or `ShadowRoot` (lib.dom): `innerHTML` / `outerHTML` assigned anything but a string or a template without substitutions; `insertAdjacentHTML`, `write`, `writeln`, `createContextualFragment`, `setHTMLUnsafe` whose last argument is not one |
| `typescript/postmessage-origin` | `ast-grep` forbid, `where` the receiver is `typescript` `Window`: `postMessage(…)` with a `"*"` argument or `{targetOrigin: "*"}` |
| `typescript/iframe-sandbox` | `tool` biome `lint/nursery/noUnsafeIframeSandbox` (tsx): no `sandbox` with both `allow-scripts` and `allow-same-origin` |
| `typescript/cors-credentials-wildcard` | `ast-grep` forbid: an options object with `credentials: true` and `origin` `"*"`, `true` or a function that returns its first parameter (as its body, with a `??` / `||` fallback, or in a `return`), passed to a function declared (§15.6) by `cors` or as `hono`'s `cors`, as the second argument of `.register()` with a first argument declared by `@fastify/cors`, to `.enableCors()` on a value of type `@nestjs/common`'s `INestApplication`, or as `cors` in the second argument of `create()` on `@nestjs/core`'s `NestFactory` |
| `typescript/no-hardcoded-session-secret` | `ast-grep` forbid, not in tests: a string or a template without `${}` (or an array of them) as `secret` in the first argument of a function declared (§15.6) as `express-session`'s `session`, `better-auth`'s `betterAuth` or `hono`'s `jwt`, or in the second argument of `.register()` with a first argument declared by `@fastify/cookie` or `@fastify/session`; as the second argument of `jsonwebtoken`'s or `hono`'s `sign` / `verify`; as the first argument of a function declared by `cookie-parser` |
| `typescript/verbatim-imports` | `tool` biome `lint/style/useImportType`, `lint/style/useExportType` (snippet: error; recommended has them as warnings) |
| `typescript/implicit-returns` | `tool` tsc: expects `TS7030` (`noImplicitReturns`) |
| `typescript/no-banned-types` | `tool` biome `lint/complexity/noBannedTypes` (snippet: error) |
| `typescript/no-const-enum` | `tool` biome `lint/suspicious/noConstEnum` (snippet: error) |
| `typescript/no-object-literal-assertion` | `ast-grep` forbid: `$E as $T` where `E` is an object literal or a non-empty array literal and `T` is not `const` |
| `typescript/no-unchecked-json` | `ast-grep` forbid: `JSON.parse(…) as T`, `const x: T = JSON.parse(…)`, and the same on `await $R.json()` where `R` is a `Response` (`typescript` lib.dom, `undici-types`); `T` other than `unknown` |
| `typescript/error-cause` | `tool` biome `lint/style/useErrorCause` |
| `typescript/array-sort-compare` | `tool` biome `lint/suspicious/useArraySortCompare` (strings too: Biome has no option to exempt them); `tool` eslint `@typescript-eslint/require-array-sort-compare` (`ignoreStringArrays: false`) |
| `typescript/no-proto` | `tool` biome `lint/suspicious/noProto` (snippet: error) |
| `typescript/no-dynamic-regexp` | `ast-grep` advise: `new RegExp($A, …)` / `RegExp($A, …)` whose pattern is not a string, a template without substitutions, or `RegExp.escape(…)` (a template whose substitutions are all `RegExp.escape(…)` counts as escaped) |

context.md: strict TypeScript; TypeScript 6 deprecations are migrated, never silenced with `ignoreDeprecations`; no `!` or `as unknown as`; `import type` for type-only imports, parsed JSON stays `unknown` until a schema validates it, `satisfies` instead of asserting a literal; search for an existing helper (warn-once, logging, `util.*`) before writing one.

### typescript/node

Detected by `@types/node` (npm).

| Rule | Targets |
|---|---|
| `typescript/node/no-shell-interpolation` | `ast-grep` forbid: a call declared (§15.6) in `node/child_process` as `exec` / `execSync` whose first argument is a template with `${}` or a `+` concatenation; a call declared there as `spawn`, `spawnSync`, `execFile`, `execFileSync` with an array second argument and `shell: true` in the third |
| `typescript/node/no-sync-fs-in-async` | `ast-grep` advise: inside an `async` function, arrow function or method, a call whose callee ends in `Sync` (Node's naming) and is declared (§15.6) in `node/fs` |
| `typescript/node/fetch-timeout` | `ast-grep` advise: a call declared (§15.6) as `fetch` by `typescript` (the DOM library), `node`, `undici-types` or `bun-types`, with one argument, or with an object literal second argument without `signal` (and no spread) |
| `typescript/node/weak-crypto` | `ast-grep` advise: a call declared (§15.6) in `node/crypto` as `createHash` / `createHmac` whose first argument is the string `md5` or `sha1` (any case) |
| `typescript/node/no-ecb-cipher` | `ast-grep` forbid: a call declared (§15.6) in `node/crypto` as `createCipheriv` / `createDecipheriv` whose first argument is a string ending in `-ecb` (any case) |
| `typescript/node/no-ts-node` | `grep` forbid in `package.json`: `ts-node` (scripts or dependencies); `guard` ask: a `ts-node` command |
| `typescript/node/no-vm-sandbox` | `ast-grep` advise: an import or `require` of `node:vm` / `vm` |
| `typescript/node/esm-globals` | `ast-grep` forbid in `@esm-modules`: `__dirname`, `__filename`, `require(…)`, `module.exports`, `exports.x = …`, where the name is declared (§15.6) by `node` (`@types/node`'s globals; a module's own `const __dirname` or `createRequire` result is not); not Biome `noGlobalDirnameFilename` / `noCommonJs`, which ignore the module type |

context.md: TypeScript runs with `node file.ts` or `tsx`, not `ts-node`; `node:vm` is not a security boundary.

### typescript/library

Detected by `library_any: [npm]` (§13.4).

| Rule | Targets |
|---|---|
| `typescript/library/deprecation-shim` | `check` lint: `forge_helper bun public_api.mjs removed` (§13.4), `when_changed`: TypeScript and JavaScript files, `package.json` |
| `typescript/library/exports-map` | `check` lint (with `bun`): a `package.json` that is not `"private": true` and has no `exports`, reported at its top-level `main`, `module`, `types` or `typings` line (so only when that line is added) |

context.md: entry points are the public API; renamed exports stay as `@deprecated` aliases until a major version.

### typescript/bun

| Rule | Targets |
|---|---|
| `typescript/bun/lock-sync` | `check` lint: `run: bun install --frozen-lockfile`, `when_changed: [package.json]` |
| `typescript/bun/no-other-pm` | `guard` ask: `npm install`, `npm i`, `npm ci`, `pnpm`, `yarn` |
| `typescript/bun/release-age` | `check` lint: `forge_bun_cooldown`: `bunfig.toml` `[install] minimumReleaseAge` ≥ 259200; `when_changed`: `bunfig.toml`, `package.json`, `bun.lock` |
| `typescript/bun/trusted-deps` | `check` lint: `forge_trusted_deps`: no `trustedDependencies` entry added to a `package.json` since the base (with `bun`) |
| `typescript/bun/test` | `check` test: `bun test` on the tracked test files that import from `bun:test` (nothing when there are none, so a Vitest suite is not run by Bun) |
| `typescript/bun/no-new-skip` | `ast-grep` no-increase on test files (`*.test.*`, `*_test.*`, `*.spec.*`, `*_spec.*`) importing `bun:test`: `it`/`test`/`describe` `.skip`, `.todo`, `.only`, `.failing`, `.if`, `.skipIf`, `.todoIf` |
| `typescript/bun/no-fewer-expects` | `ast-grep` no-decrease on the same files: `expect($$$)` |
| `typescript/bun/no-mock-increase` | `ast-grep` no-increase on the same files: `mock(…)`, `mock.module(…)`, `spyOn(…)` (also under an import alias), `jest.fn/mock/spyOn`, `vi.fn/mock/spyOn` |
| `typescript/bun/no-test-edits` | `check` lint: `forge_test_edits` on the in-scope files that import `bun:test` (§10.5) |
| `typescript/bun/no-snapshot-update` | `guard` ask: `bun test` with `-u` or `--update-snapshots` |
| `typescript/bun/no-shell-raw` | `ast-grep` forbid: a tagged template whose tag is declared (§15.6) as `bun-types`' `$` with an `${{ raw: … }}` substitution; a call declared as `bun-types`' `spawn` / `spawnSync` whose first argument is an array `["sh" \| "bash" \| "zsh", "-c", <template with ${} or + concatenation>]` |
| `typescript/bun/text-lockfile` | `check` lint: no `bun.lockb` file (tracked or untracked, not ignored); `when_changed`: `bun.lockb`, `package.json` |

context.md: Bun is the runtime and package manager; `bun:test` tests run with `bun test`, what the integrity rules reject in them, and "If an existing test looks wrong, stop and ask; never edit it to make the code pass."

### typescript/eslint

Detected by an `eslint.config.*` file or the `eslint` dependency at 9 or later.

| Rule | Targets |
|---|---|
| `typescript/eslint/flat-config-only` | `check` lint: no `.eslintrc`, `.eslintrc.*` or `.eslintignore` file (tracked or untracked, not ignored); `when_changed`: those names |
| `typescript/eslint/no-eslint-env` | `grep` forbid: `/* eslint-env` comments |
| `typescript/eslint/lint` | `check` lint, `fail_on_warnings`: `run: eslint --max-warnings=0 .`, `changed_run: eslint --max-warnings=0 --no-warn-ignored --no-error-on-unmatched-pattern {files}`; ESLint's stylish output is attributed by file and line (§10.1) |
| `typescript/eslint/type-checked` | `tool` eslint: typescript-eslint's `no-unsafe-assignment`, `no-unsafe-call`, `no-unsafe-member-access`, `no-unsafe-return`, `no-unsafe-argument`, `return-await`, `unbound-method`, `no-misused-spread`, `no-array-delete`, `no-for-in-array`, `restrict-template-expressions`, `prefer-promise-reject-errors`, `use-unknown-in-catch-callback-variable`, `no-unnecessary-type-assertion`, `no-unnecessary-condition`, `consistent-type-assertions` (`objectLiteralTypeAssertions: never`); snippet: error; type-aware (`parserOptions.projectService`) |
| `typescript/eslint/ts-version` | `check` lint (with `bun`): the `typescript` that `@typescript-eslint/typescript-estree` resolves (directly, or through `typescript-eslint` / `@typescript-eslint/parser`) exports `createProgram`; TypeScript 7 has no compiler API, typescript-eslint supports `<6.1.0`; `when_changed`: `package.json` files and lock files |

context.md: flat config only (ignores and globals in `eslint.config.*`); typescript-eslint needs TypeScript 6 or older; warnings block; the type-aware rules are errors.

### typescript/oxlint

Detected by `.oxlintrc.json`, `oxlint.config.ts` or the `oxlint` dependency.

| Rule | Targets |
|---|---|
| `typescript/oxlint/lint` | `check` lint, `fail_on_warnings`: `run: oxlint --type-aware --deny-warnings --format unix`, `changed_run` the same on `{files}`; without `oxlint-tsgolint` the type-aware run fails ("Failed to find tsgolint executable"), so the type-aware rules are never skipped silently |

context.md: `--type-aware` with oxlint-tsgolint; warnings block.

### typescript/prettier

Detected by a Prettier config file (`.prettierrc*`, `prettier.config.*`) or the `prettier` dependency.

| Rule | Targets |
|---|---|
| `typescript/prettier/format` | `check` format: `run: prettier --check .`, `changed_run: prettier --write --ignore-unknown {files}`, `format_stdin: prettier --stdin-filepath {file}`; Prettier's `[warn] <path>` and `[error] <path>: …` lines name the files (§10.1) |

context.md: Prettier formats; its options live in its config.

### typescript/pnpm

Detected by `pnpm-lock.yaml` or `pnpm-workspace.yaml`.

| Rule | Targets |
|---|---|
| `typescript/pnpm/no-other-pm` | `guard` ask: `npm install`, `npm i`, `npm ci`, `npm add`, `yarn`, `bun install`, `bun i`, `bun add` |
| `typescript/pnpm/lock-sync` | `check` lint: `forge_do pnpm lock-check` (`pnpm install --frozen-lockfile --lockfile-only`; not enforced before pnpm 8.6.12, which rewrites a stale lock; skipped when pnpm is not installed), `when_changed`: `package.json` files, `pnpm-lock.yaml`, `pnpm-workspace.yaml` |
| `typescript/pnpm/settings-location` | `check` lint (with `bun`) on changed `package.json` and `.npmrc` files: a top-level `pnpm` field, and pnpm settings in `.npmrc` (`node-linker`, `shamefully-hoist`, `only-built-dependencies`, `minimum-release-age`, …), reported at their line |
| `typescript/pnpm/no-removed-settings` | `grep` forbid in `pnpm-workspace.yaml`, `when` `pnpm >= 11` (`packageManager`): the top-level keys `onlyBuiltDependencies`, `neverBuiltDependencies`, `ignoredBuiltDependencies`, `ignoreDepScripts`, `allowNonAppliedPatches`, and `ignoreCves` (under `auditConfig`) |
| `typescript/pnpm/allow-builds` | `check` lint: `.forge/helpers/pm_policy.mjs` (`forge_helper bun pm_policy.mjs <mode>`) `pnpm-builds`, when `pnpm-workspace.yaml` changed, compared with the base: an `allowBuilds` entry newly `true`, a new `onlyBuiltDependencies` entry, `dangerouslyAllowAllBuilds: true`, `strictDepBuilds` or `blockExoticSubdeps` turned `false`; each at its line, accepted in `forge.yaml` |
| `typescript/pnpm/workspace-protocol` | `check` lint: `pnpm-workspace`: in a changed `package.json`, a dependency on a package of the workspace (`pnpm-workspace.yaml` `packages`, `!` exclusions applied) whose spec has no protocol (`^1.2.0`, not `workspace:`), at its line |
| `typescript/pnpm/catalog` | `check` lint, advisory (§10.7): `pnpm-catalog`: a dependency added or changed in a `package.json` whose name is a key of `catalog` or of a `catalogs` entry and whose spec is not `catalog:…` / `workspace:…`; nothing when `catalogMode: strict` |
| `typescript/pnpm/overrides` | `check` lint, advisory (§10.7): `pnpm-overrides`: each `overrides` entry of `pnpm-workspace.yaml` added or changed since the base |

context.md: pnpm manages dependencies; its settings live in `pnpm-workspace.yaml`; pnpm 11's `allowBuilds`, `allowUnusedPatches`, `auditConfig.ignoreGhsas`.

### typescript/npm

Detected by `package-lock.json` or `npm-shrinkwrap.json`.

| Rule | Targets |
|---|---|
| `typescript/npm/no-other-pm` | `guard` ask: `pnpm`, `yarn`, `bun install`, `bun i`, `bun add` |
| `typescript/npm/lock-sync` | `check` lint: `forge_do npm lock-check` (`npm ci --dry-run --ignore-scripts --no-audit --no-fund` from npm 10.5.1; before, `npm ls --package-lock-only --all`, since `ci --dry-run` deletes `node_modules` there; skipped when npm is not installed), `when_changed`: `package.json` files, `package-lock.json`, `npm-shrinkwrap.json` |
| `typescript/npm/no-exotic-sources` | `grep` forbid in `.npmrc`: `allow-git=all`, `allow-remote=all`; `guard` deny: the same settings in a command |
| `typescript/npm/no-exotic-deps` | `check` lint: a `dependencies`, `devDependencies` or `optionalDependencies` entry added or changed since the base (`forge_pkg_deps`) whose spec is `git+…`, `git:`, `github:`, `gitlab:`, `bitbucket:`, `http(s)://…` or `user/repo` |
| `typescript/npm/no-legacy-peer-deps` | `guard` deny: `npm install`, `i`, `ci`, `add` with `--legacy-peer-deps` or `--force` |
| `typescript/npm/no-legacy-peer-deps-config` | `grep` forbid in `.npmrc`: `legacy-peer-deps=true`, `force=true` |
| `typescript/npm/no-audit-fix-force` | `guard` ask: `npm audit fix … --force` |
| `typescript/npm/release-age` | `check` lint: `.forge/helpers/pm_policy.mjs` (`forge_helper bun pm_policy.mjs <mode>`) `npm-cooldown`, when `.npmrc`, `package.json` or `package-lock.json` changed: root `.npmrc` `min-release-age` is at least 3 (days), and `package.json` requires npm ≥ 11.10 (`devEngines.packageManager` `{name: npm}` version range, or `packageManager` `npm@…`), the first npm that reads it |
| `typescript/npm/release-age-command` | `guard` ask: `--min-release-age=0` (or `0` as the next word) |
| `typescript/npm/allow-scripts` | `check` lint: `npm-scripts`, compared with the base: root `package.json` `allowScripts` gains an entry that is not `false` (or one turns from `false`); `.npmrc` gains `ignore-scripts=false`, `dangerously-allow-all-scripts=true` or an `allow-scripts` package; accepted in `forge.yaml` |
| `typescript/npm/allow-scripts-command` | `guard` ask: `npm approve-scripts` / `npm install-scripts approve` with a package, `--all` or `--no-allow-scripts-pin` (not the read-only `--allow-scripts-pending`), `--allow-scripts`, `--dangerously-allow-all-scripts` |
| `typescript/npm/lockfile-lint` | `check` lint: `forge_lockfile_lint` on changed `package-lock.json` files (`@lockfiles`): pinned lockfile-lint 5.1.0 through `bunx`, `--type npm --allowed-hosts npm --validate-https --validate-integrity`; findings `path:0 <package>: <problem> (expected …, actual …)`; a private registry replaces the rule locally |

context.md: npm manages dependencies; peer conflicts are resolved, not forced; dependencies come from the registry.

### typescript/yarn

Detected by `.yarnrc.yml` (Yarn 2 and later).

| Rule | Targets |
|---|---|
| `typescript/yarn/no-other-pm` | `guard` ask: `npm install`, `npm i`, `npm ci`, `npm add`, `pnpm`, `bun install`, `bun i`, `bun add` |
| `typescript/yarn/scripts-ratchet` | `check` lint: `.forge/helpers/pm_policy.mjs` (`forge_helper bun pm_policy.mjs <mode>`) `yarn-scripts`, compared with the base: `.yarnrc.yml` `enableScripts` turned `true`; a `package.json` `dependenciesMeta` entry gaining `built: true`; accepted in `forge.yaml` |
| `typescript/yarn/lock-sync` | `check` lint: `yarn install --immutable --mode=skip-build` (skipped when yarn is not installed or is Yarn 1, §13.5), `when_changed`: `package.json` files, `yarn.lock`, `.yarnrc.yml` |

context.md: Yarn manages dependencies.

### typescript/turbo

Detected by `turbo.json`.

| Rule | Targets |
|---|---|
| `typescript/turbo/tasks-key` | `grep` forbid in `turbo.json`: a `"pipeline"` key (Turborepo 2 uses `tasks`) |
| `typescript/turbo/declared-env` | `tool` biome `lint/suspicious/noUndeclaredEnvVars` (snippet: error; Biome's turborepo domain enables it as a warning only) |
| `typescript/turbo/no-env-loose` | `grep` forbid in `turbo.json`: `"envMode": "loose"` |
| `typescript/turbo/no-env-loose-flag` | `guard` deny: `--env-mode=loose`, `--env-mode loose` |

context.md: `tasks`, strict env mode, every env var read is declared in `env` or `globalEnv`.

### typescript/jest

Detected by the `jest` dependency at 30 or later.

| Rule | Targets |
|---|---|
| `typescript/jest/no-test-path-pattern` | `grep` forbid in `package.json`: `--testPathPattern` (Jest 30: `--testPathPatterns`); `guard` deny: a `jest` command with it |

### typescript/testing-library

Detected by the `@testing-library/react` dependency.

| Rule | Targets |
|---|---|
| `typescript/testing-library/no-hooks-lib` | `ast-grep` forbid: an import or `require` of `@testing-library/react-hooks` |
| `typescript/testing-library/no-container-queries` | `ast-grep` advise in `@ts-tests`: `$R.container.querySelector(…)` / `querySelectorAll(…)` and `const { container } = $R`, where `R` is `@testing-library/react`'s `RenderResult` (§15.1: an alias of an intersection) |
| `typescript/testing-library/lint` | `tool` eslint (test files, `@ts-tests`): `testing-library/await-async-queries`, `no-wait-for-side-effects`, `prefer-find-by`, `prefer-screen-queries`, `no-unnecessary-act`, `no-node-access` (snippet: error; eslint-plugin-testing-library) |

context.md: `renderHook` from `@testing-library/react`; the testing-library ESLint rules' practices.

### typescript/msw

Detected by the `msw` dependency.

| Rule | Targets |
|---|---|
| `typescript/msw/unhandled-error` | `ast-grep` advise: `$S.listen(…)` without `onUnhandledRequest: "error"` (or a function), where `S` is msw's `SetupServer` / `SetupServerCommon` (§15.1) |

context.md: the test server starts with `onUnhandledRequest: "error"`.

### typescript/storybook

Detected by the `storybook` dependency at 9 or later.

| Rule | Targets |
|---|---|
| `typescript/storybook/consolidated-imports` | `ast-grep` forbid: imports and re-exports from `@storybook/test`, `addon-actions`, `addon-viewport`, `addon-essentials`, `addon-interactions`, `manager-api`, `preview-api`, `theming`, `blocks` |
| `typescript/storybook/consolidated-deps` | `grep` forbid in `package.json`: those packages as keys |

context.md: the `storybook/*` entry points that replace them.

### typescript/vite

Detected by the `vite` dependency.

| Rule | Targets |
|---|---|
| `typescript/vite/no-public-secret-env` | `ast-grep` forbid: `import.meta.env.VITE_*` names containing `SECRET`, `PRIVATE`, `PASSWORD`, `SERVICE_ROLE` or `DATABASE_URL` (the `typescript/nextjs/no-public-secret-env` list) |
| `typescript/vite/no-public-secret-env-file` | `grep` forbid in `.env` and `.env.*` files: such `VITE_` variables |
| `typescript/vite/no-define-process-env` | `ast-grep` forbid in `vite.config.*`: a `"process.env"` key of `define` set to `process.env` or `JSON.stringify(process.env)` |
| `typescript/vite/no-open-dev-host` | `ast-grep` forbid in `vite.config.*`: `allowedHosts: true` under `server` or `preview` |

context.md: no secrets in `VITE_` variables, `define` never inlines `process.env`, `allowedHosts` lists hosts.

### typescript/t3-env

Detected by `@t3-oss/env-core`, `@t3-oss/env-nextjs` or `@t3-oss/env-nuxt`.

| Rule | Targets |
|---|---|
| `typescript/t3-env/no-process-env` | `tool` biome `lint/style/noProcessEnv` (snippet: error, off in `env.ts`, `env.mjs`, `env.js` and `*.config.*`, a convention the project edits in its biome.json) |

context.md: read env vars through the validated `env`.

### typescript/nextjs

| Rule | Targets |
|---|---|
| `typescript/nextjs/no-pages-data` | `ast-grep` forbid in `app/**`, `src/app/**`: `getServerSideProps`, `getStaticProps` exports |
| `typescript/nextjs/no-next-router` | `ast-grep` forbid: `import $$$ from "next/router"` |
| `typescript/nextjs/proxy-not-middleware` | `ast-grep` forbid: any code in `middleware.ts` / `src/middleware.ts` (file renamed to `proxy.ts` in Next.js 16) |
| `typescript/nextjs/revalidate-tag-profile` | `ast-grep` forbid: `revalidateTag($A)` with one argument |
| `typescript/nextjs/no-legacy-image-config` | `ast-grep` forbid: imports of `next/legacy/image`; a `domains` key inside `images` |
| `typescript/nextjs/no-public-secret-env` | `ast-grep` forbid: `process.env.NEXT_PUBLIC_*` names containing `SECRET`, `PRIVATE`, `PASSWORD`, `SERVICE_ROLE` or `DATABASE_URL` |
| `typescript/nextjs/proxy-exports` | `ast-grep` forbid in `proxy.ts` / `src/proxy.ts`: a named export `middleware` (declared, or `export { x as middleware }`), an exported `runtime` |
| `typescript/nextjs/no-ignore-build-errors` | `ast-grep` forbid in `next.config.*`: `ignoreBuildErrors: true` under `typescript` |
| `typescript/nextjs/no-request-apis-in-use-cache` | `ast-grep` forbid, in a file importing `next/headers`: `cookies()`, `headers()`, `draftMode()` inside a function or file whose first statement is `'use cache'` (or `'use cache: <profile>'`) |
| `typescript/nextjs/no-redirect-in-try` | `ast-grep` forbid, in a file importing `next/navigation`: `redirect`, `permanentRedirect`, `notFound`, `forbidden`, `unauthorized` called in a `try` block (same function) whose `catch` does not call `unstable_rethrow(…)` |
| `typescript/nextjs/stable-cache-imports` | `ast-grep` forbid: `unstable_cacheLife`, `unstable_cacheTag` imported from `next/cache` |
| `typescript/nextjs/no-next-head-in-app` | `ast-grep` forbid in `app/**`, `src/app/**`: imports of `next/head`, `next/document` |
| `typescript/nextjs/image-remote-patterns-strict` | `ast-grep` forbid in `next.config.*`: `hostname: "*"` or `"**"` (or `*.*`, …) under `remotePatterns` |
| `typescript/nextjs/image-unsafe-options` | `ast-grep` forbid in `next.config.*`, under `images`: `dangerouslyAllowSVG: true` without a sibling `contentSecurityPolicy`; `dangerouslyAllowLocalIP: true` |
| `typescript/nextjs/no-sync-scripts` | `tool` biome `lint/performance/noSyncScripts` (tsx) |
| `typescript/nextjs/parallel-route-default` | `check` lint, scope `app/**`, `src/app/**`: every `@*` directory under `app/` or `src/app/` has a `default.tsx`, `.ts`, `.jsx` or `.js` |
| `typescript/nextjs/cookies-set-in-render` | `ast-grep` forbid in `page.*`, `layout.*`, `template.*`, `default.*` under `app/**`, `src/app/**`, in a file importing `next/headers` and not starting with `'use server'`: `.set(…)` / `.delete(…)` on `(await cookies())` or on a name bound to `await cookies()` in the same block, outside a function whose first statement is `'use server'` |

context.md: Next.js 16 App Router: async request APIs, Server Components by default, `proxy.ts`, `revalidateTag(tag, profile)` / `updateTag(tag)`, `images.remotePatterns` without wildcard hosts, no `next lint`, Server Actions and route handlers are public endpoints (guidance only: validating input and checking the caller cannot be decided from syntax, so no rule checks it), no secrets in `NEXT_PUBLIC_`, no request APIs in `'use cache'`, no `redirect()` swallowed by a `try`, no `ignoreBuildErrors`, a `default.tsx` per `@slot`, the Metadata API.

### typescript/nextjs/react

| Rule | Targets |
|---|---|
| `typescript/nextjs/react/client-hooks` | `ast-grep` forbid in `app/**`, `src/app/**`: client-only hooks (`useState`, `useEffect`, `useRef`, `useContext`, …) in a file without `'use client'`, except inside a custom hook (a function named `use…`) |
| `typescript/nextjs/react/no-server-in-client` | `ast-grep` forbid in `app/**`, `src/app/**`, `components/**`, `src/components/**` (tsx): a file starting with `'use client'` that imports `server-only`, `node:*`, `fs`, `child_process`, `next/headers`, `next/root-params`, `@prisma/client`, `@prisma/adapter-*` or a `generated/prisma` path |
| `typescript/nextjs/react/client-env-public-only` | `ast-grep` forbid in a file starting with `'use client'`: `process.env.X` other than `NEXT_PUBLIC_*` and `NODE_ENV` |
| `typescript/nextjs/react/no-ssr-false-in-server` | `ast-grep` forbid in `app/**`, `src/app/**`, in a file importing `next/dynamic` and not starting with `'use client'`: `ssr: false` in the arguments of `dynamic(…)` |
| `typescript/nextjs/react/no-async-client-component` | `tool` biome `lint/correctness/noNextAsyncClientComponent` (tsx) |
| `typescript/nextjs/react/server-functions-async` | `ast-grep` forbid in a file starting with `'use server'`: an export (named or default) of a non-async function or arrow, a class, or a literal, object, array or `new` value; a call or a name is not judged (it may return an async function) |

context.md: "React 19 in the App Router: components are Server Components unless their file starts with `'use client'`. State, effects, context, refs and event handlers need a Client Component; keep `'use client'` at the leaves. Server Actions are async functions marked `'use server'`. A `'use client'` file reads only `NEXT_PUBLIC_*` and `NODE_ENV` from `process.env` and exports no async component; a `'use server'` file exports only async functions."

### typescript/nextjs/cache-components

Detected by a `next.config.{js,mjs,cjs,ts,mts}` line matching `^\s*cacheComponents\s*:\s*true\b` (`content_any`, §5.1; `packs.add` / `packs.remove` override it).

| Rule | Targets |
|---|---|
| `typescript/nextjs/cache-components/no-segment-config` | `ast-grep` forbid in `app/**`, `src/app/**`: an exported `const` named `dynamic`, `revalidate`, `fetchCache` or `experimental_ppr` (`next build` rejects them under `cacheComponents`) |
| `typescript/nextjs/cache-components/no-unstable-cache` | `ast-grep` advise: `unstable_cache` imported from `next/cache` (use `'use cache'` with `cacheTag()` / `cacheLife()`) |

context.md: caching with `'use cache'`, `cacheLife()`, `cacheTag()`; no segment config exports; `<Suspense>` around request data.

### typescript/react

Detected by `react >= 19` (npm); a sibling of `nextjs` and `astro`, so it applies to every React setup.

| Rule | Targets |
|---|---|
| `typescript/react/no-forward-ref` | `tool` biome `lint/suspicious/noReactForwardRef`; `ast-grep` forbid calls to `forwardRef` / `React.forwardRef` (tsx) |
| `typescript/react/no-prop-types` | `ast-grep` forbid: `$C.propTypes = $V`, `$C.defaultProps = $V` |
| `typescript/react/use-action-state` | `ast-grep` forbid: `useFormState` imported from `react-dom` |
| `typescript/react/no-dangerous-html` | `tool` biome `lint/security/noDangerouslySetInnerHtml`; `ast-grep` forbid the `dangerouslySetInnerHTML` attribute |
| `typescript/react/hooks-rules` | `tool` biome `lint/correctness/useHookAtTopLevel`, `lint/correctness/useExhaustiveDependencies` |
| `typescript/react/stable-keys` | `tool` biome `lint/suspicious/noArrayIndexKey` |
| `typescript/react/no-nested-components` | `tool` biome `lint/correctness/noNestedComponentDefinitions`, `lint/nursery/noComponentHookFactories` (a function that defines and returns a component or hook) |
| `typescript/react/a11y` | deprecated, replaced by `typescript/a11y` (which covers the Vue, Svelte, Astro and HTML templates too) |
| `typescript/react/no-deprecated-types` | deprecated, replaced by `typescript/no-deprecated` (every deprecated use, React's types included, through the project's TypeScript) |
| `typescript/react/compiler-rules` | `tool` biome `lint/nursery/useReactCompiler` (snippet: error, `compilationMode: infer`): no synchronous `setState` in an effect, no ref read or written while rendering, no impure call while rendering |
| `typescript/react/no-prop-assign` | `tool` biome `lint/correctness/noReactPropAssignments` |
| `typescript/react/stable-default-props` | `tool` biome `lint/nursery/noReactObjectTypeAsDefaultProp`: no object, array or function literal as a destructured prop's default |
| `typescript/react/act-from-react` | `ast-grep` forbid: imports from `react-dom/test-utils` |

context.md: React 19: `ref` is a prop (no `forwardRef`), `useActionState` and `act` from `react`, no `propTypes` / `defaultProps` (object defaults hoisted), `createRoot` instead of `ReactDOM.render`, React Compiler's checks, props never assigned to.

### typescript/react/zustand

Detected by `zustand >= 5` (npm), under `typescript/react`.

| Rule | Targets |
|---|---|
| `typescript/react/zustand/stable-selector` | `ast-grep` forbid: a call whose first argument is an arrow returning an object or array literal (`(s) => ({ … })`, `(s) => [ … ]`, or a block returning one), where the callee is `zustand`'s `UseBoundStore` (§15.1: an alias of an intersection); a selector wrapped in `useShallow(…)` is not an arrow argument |

context.md: zustand 5 compares selector results with `Object.is`: one value per selector, or `useShallow`.

### typescript/react/react-hook-form

Detected by `react-hook-form >= 7` (npm), under `typescript/react`.

| Rule | Targets |
|---|---|
| `typescript/react/react-hook-form/watch-all` | `ast-grep` advise: `$W()` with no argument where `W` is `react-hook-form`'s `UseFormWatch` (§15.1: an alias of an object type literal), however `watch` is destructured or renamed |

### typescript/astro

| Rule | Targets |
|---|---|
| `typescript/astro/check` | `check` typecheck: `forge_do astro check`; without `@astrojs/check` (Astro 4 and later print why and exit 0) it is reported skipped, never passed; scope `**/*.astro`, `**/*.ts`, `**/*.tsx` |
| `typescript/astro/no-set-html` | `grep` forbid in `**/*.astro`: `set:html=` |

context.md: Astro renders on the server to static HTML; `astro check`; content collections in `src/content.config.ts` with `glob()` / `file()` loaders and `entry.id`; no `Astro.glob()`; `<ClientRouter />`.

### typescript/astro/react

context.md: "React components in Astro render to static HTML; they become interactive only with a `client:*` directive. Hooks and event handlers run only in hydrated islands. Props passed to a hydrated island must be serializable."

### typescript/prisma

| Rule | Targets |
|---|---|
| `typescript/prisma/validate` | `check` migration: `run: prisma validate`, `when_changed: [prisma/schema.prisma]` |
| `typescript/prisma/migration-present` | `check` migration: `forge_do prisma migrations-check` (`prisma migrate diff --from-migrations prisma/migrations --to-schema prisma/schema.prisma --exit-code`, Prisma 7 and later): exit 2 (a difference) fails, any other failure is reported skipped; `when_changed: [prisma/schema.prisma]`, `requires_local_db: true`; skipped with a message if `prisma.config.ts` has no `shadowDatabaseUrl`; passes in a db push workflow (no `prisma/migrations` directory and `prisma db push` in `package.json`) |
| `typescript/prisma/no-reset` | `guard` deny: `prisma migrate reset`, `prisma db push --force-reset`, `prisma db push --accept-data-loss` |
| `typescript/prisma/db-push` | `guard` ask: `prisma db push`, `prisma migrate deploy`, `prisma db execute` |
| `typescript/prisma/generated-client-import` | `ast-grep` forbid: `import $$$ from "@prisma/client"` |
| `typescript/prisma/no-middleware` | `ast-grep` forbid: `$C.$use($$$)` |
| `typescript/prisma/no-raw-unsafe` | `ast-grep` forbid: `$C.$queryRawUnsafe($$$)`, `$C.$executeRawUnsafe($$$)` |
| `typescript/prisma/no-raw-interpolation` | `ast-grep` forbid: a call `$CALL` of `raw(…)` / `<x>.raw(…)` whose argument is a template with `${}`, a `+` concatenation or a variable declared from one (the `drizzle/no-sql-raw` matcher), where `CALL` is the runtime's `Sql` (`@prisma/client`, `@prisma/client-runtime-utils`): the generated client's path and import names never matter |
| `typescript/prisma/no-unscoped-bulk-write` | `ast-grep` advise: `.deleteMany(…)` / `.updateMany(…)` without an object argument holding `where`, where the call is the runtime's `PrismaPromise` (`@prisma/client`; generated delegates are project code) |
| `typescript/prisma/client-generator` | `check` lint: no `provider = "prisma-client-js"` in tracked `*.prisma` files, `when_changed: [**/*.prisma]` |
| `typescript/prisma/migration-lint` | `check` migration: changed `prisma/migrations/*/migration.sql` linted with Squawk (§13.1); `--all`: the newest migration; `when_changed: [prisma/migrations/**]` |
| `typescript/prisma/no-network-in-transaction` | `ast-grep` forbid: inside a function passed to `.$transaction(…)`, a call to the global `fetch` / `globalThis.fetch` / `$fetch`, or to a name (or a method of a name) imported from `axios`, `got`, `ky`, `ofetch`, `node-fetch` or `undici` |
| `typescript/prisma/destructive-migration` | `check` migration: `DROP COLUMN` / `DROP TABLE` statements (not `--` or `/* */` comments) in the changed `prisma/migrations/*/migration.sql` (`--all`: the newest), read by `forge_prisma_drops`, which follows each table's columns through every `migration.sql`: a SQLite redefinition (`CREATE TABLE "new_X"`, `DROP TABLE "X"`, `RENAME TO "X"`) is reported only for the columns `new_X` lacks (`… (redefinition drops column "C")`), and not when no earlier migration creates `X`; printed `file:line: <statement>`; a line found in `forge.yaml` (an `accept` entry) passes; `when_changed: [prisma/migrations/**]` |
| `typescript/prisma/no-query-in-loop` | `ast-grep` advise: `await $Q` in the body of a `for`, `for…of`, `for…in`, `while` or `do`, or in a function passed to `.map` / `.forEach` / `.flatMap`, where `Q` is `@prisma/client`'s `PrismaPromise` (§15.1): `findMany`, `count`, `aggregate`, `groupBy`, `*Many`, and the `Prisma__<Model>Client` of `findUnique`, `findFirst`, `create`, `update`, `upsert`, `delete`, which extends the generated client's type alias of it |
| `typescript/prisma/applied-migrations` | `check` migration: `forge_applied_migrations @prisma-migrations`: a migration that exists in the base is not modified or deleted (not with `--all`) |

context.md: Prisma 7: `prisma-client` generator with `output`, driver adapters, `prisma.config.ts` (no automatic `.env`), `prisma db seed`, `$extends` (no `$use`), tagged `$queryRaw` only (no `Prisma.raw()` from a template); no network calls in interactive transactions; migrations with schema changes (not under db push); drops accepted in `forge.yaml`.

### typescript/vitest

| Rule | Targets |
|---|---|
| `typescript/vitest/run` | `check` test: `run: vitest run`, `changed_run: vitest run --changed`; timeout 300 s |
| `typescript/vitest/no-new-skip` | `ast-grep` no-increase on test files: `it.skip`, `test.skip`, `describe.skip`, `it.todo`, `test.todo`, `describe.todo`, `$X.skipIf`, `$X.runIf`, `$X.only` |
| `typescript/vitest/no-fewer-expects` | `ast-grep` no-decrease on test files: `expect($$$)` |
| `typescript/vitest/no-test-env-branch` | `ast-grep` forbid on non-test files: `process.env.VITEST`, `process.env.NODE_ENV === "test"` |
| `typescript/vitest/no-tautological-expect` | `ast-grep` forbid on test files: `expect(<literal>).$M(…)` (`true`, `false`, `null`, `undefined`, a number or a string) |
| `typescript/vitest/no-matcher-override` | `ast-grep` forbid, in `@ts-tests`, `vitest.setup.*` and `__mocks__/`: `$E.extend({…})` defining a built-in matcher name (`toBe`, `toEqual`, `toStrictEqual`, `toMatchObject`, `toThrow`, … the names of `@vitest/expect`'s assertion interfaces and the snapshot matchers) where `E` is Vitest's `ExpectStatic` (declared in `vitest` from 5, `@vitest/expect` before; §15.1) |
| `typescript/vitest/no-exit-zero` | `ast-grep` forbid on test, setup and mock files: `process.exit(0)` |
| `typescript/vitest/no-snapshot-update` | `guard` ask: `vitest` with `-u` or `--update` |
| `typescript/vitest/no-workspace-file` | `check` lint: no tracked `vitest.workspace.*` (Vitest 4 ignores it), `when_changed: [vitest.*]` |
| `typescript/vitest/no-pass-with-no-tests` | `guard` ask: `vitest … --passWithNoTests` |
| `typescript/vitest/config-file` | `permission` ask: `Edit(**/vitest.config.*)` |

| `typescript/vitest/no-mock-increase` | `ast-grep` no-increase on test files: `vi.mock`, `vi.doMock`, `vi.spyOn`, `vi.fn` calls, also through `import { vi as <alias> }`; `vi.stubEnv` / `vi.stubGlobal` are not counted |
| `typescript/vitest/no-test-edits` | `check` lint: `forge_test_edits` on `**/*.test.*`, `**/*.spec.*`, `**/__tests__/**`, `**/__mocks__/**`, `**/test-utils.*`, `**/test-helpers.*`, `**/vitest.setup.*`, not `e2e/**`; edits ask (§10.5); `review` as pytest's |
| `typescript/vitest/untested-change` | `check` lint, advisory (§10.7): `forge_untested_change` on non-test TypeScript and JavaScript files (not `*.d.ts`, `*.config.*`, `e2e/**`) |
| `typescript/vitest/snapshot-stale` | `check` lint, advisory (§10.7, `forge_once`): `forge_snapshot_stale`, Stop only (`--turn`): a changed non-test source file whose added lines hold a string or template literal in an expression (not an import or export source, a type annotation, a literal type, an interface or a type alias) or an object literal key (`pair`, shorthand property; ast-grep, `FORGE_SNAP_VALUES`), imported directly (relative import) by a test that uses `toMatchSnapshot`, `toMatchFileSnapshot`, `toMatchInlineSnapshot` or golden files (`testdata/`, `golden`), while no `.snap`, `__snapshots__/`, `testdata/` or golden file changed in the turn (a test using only inline snapshots counts as updated when it changed). It blocks once per turn, then prints `accepted` and passes (`forge_once`) |

context.md: Vitest 4 `test.projects` and `maxWorkers`; assertions are never weakened to make a test pass; "When the task changes behaviour an existing test asserts, update that test in place with Edit or Write (keep its name; Forge asks for approval) and say so in your summary. Otherwise never edit an existing test to make the code pass."

### typescript/playwright

| Rule | Targets |
|---|---|
| `typescript/playwright/run` | `check` test: `run: playwright test --reporter=line`, `when_changed: [e2e/**, playwright.config.*]` |
| `typescript/playwright/no-new-skip` | `ast-grep` no-increase on `e2e/**`: `test.skip`, `test.fixme`, `test.fail`, `test.only`, `test.describe.skip`, `test.describe.fixme`, `test.describe.only` |
| `typescript/playwright/no-wait-for-timeout` | `ast-grep` forbid in `e2e/**`, `**/*.spec.*`: `$P.waitForTimeout($$$)` |
| `typescript/playwright/locators` | `ast-grep` forbid in `e2e/**`, `**/*.spec.*`: `$`, `$$`, `$eval`, `$$eval`, `waitForSelector`, `waitForNavigation` calls |
| `typescript/playwright/no-snapshot-update` | `guard` ask: `playwright test` with `--update-snapshots` or `-u` |
| `typescript/playwright/config-file` | `permission` ask: `Edit(**/playwright.config.*)` |

| `typescript/playwright/axe` | `check` lint: when `package.json` names `@axe-core/playwright`, a tracked `e2e/*` or `*.spec.*` file uses `AxeBuilder` |
| `typescript/playwright/no-test-edits` | `check` lint: `forge_test_edits` on `e2e/**`; edits ask (§10.5); `review` as pytest's |

context.md: locators (`getByRole`, `getByLabel`, `getByText`, `getByTestId`) and web-first assertions; never `waitForTimeout`; `@axe-core/playwright` for accessibility scans; "When the task changes behaviour an existing test asserts, update that test in place with Edit or Write (keep its name; Forge asks for approval) and say so in your summary. Otherwise never edit an existing test to make the code pass."

### typescript/zod

| Rule | Targets |
|---|---|
| `typescript/zod/v4-string-formats` | `ast-grep` forbid: a string-format method (`email`, `uuid`, `url`, `datetime`, …) called on a chain rooted at `z.string()` |
| `typescript/zod/error-param` | `ast-grep` forbid: a `message`, `required_error`, `invalid_type_error` or `errorMap` key in an object passed to a call on a `z.` chain |

context.md: Zod at boundaries with `z.infer`; Zod 4 top-level formats, the single `error` parameter, `z.strictObject` / `z.looseObject`, `error.issues`, `z.treeifyError`.

### typescript/drizzle

Detected by `drizzle.config.*` or the `drizzle-orm` / `drizzle-kit` dependency. The checks read `schema`, `out` (default `./drizzle`) and `dialect` from `drizzle.config.*` (string values, arrays, globs and directories; values computed at run time are not read).

| Rule | Targets |
|---|---|
| `typescript/drizzle/migration-present` | `check` migration: `forge_drizzle_migration_present`: a changed schema file whose content reaching the database changed (blank lines, comments, imports, type exports, `relations()` / `defineRelations()` calls aside) needs, among the changed files, a `.sql` in `out` plus `meta/_journal.json` and a `meta/*_snapshot.json`, or (drizzle-kit 1.x) `<out>/<name>/migration.sql` and a `snapshot.json`; not with `--all` |
| `typescript/drizzle/applied-migrations` | `check` migration: `forge_drizzle_applied`: no `.sql` or snapshot in `out` that exists in the base is modified or deleted |
| `typescript/drizzle/migration-lint` | `check` migration: with `dialect: "postgresql"`, changed `.sql` files in `out` linted with Squawk (§13.1); `--all`: the newest one; `when_changed: [**/*.sql]` |
| `typescript/drizzle/meta-files` | `permission` deny: `Edit(**/meta/_journal.json)`, `Edit(**/meta/*_snapshot.json)` |
| `typescript/drizzle/no-push` | `guard` ask: `drizzle-kit push` |
| `typescript/drizzle/no-push-force` | `guard` deny: `drizzle-kit push … --force` (auto-approves data-loss statements) |
| `typescript/drizzle/write-with-where` | `ast-grep` forbid: `$DB.delete(…)` / `$DB.update(…)` with no `.where` further along its method chain, where `DB` is a Drizzle database (`PgDatabase`, `MySqlDatabase`, `BaseSQLiteDatabase`, `SingleStoreDatabase`, `GelDatabase`; transactions extend them; `drizzle()`'s intersection `NodePgDatabase & { $client }` by its member, §15.1) |
| `typescript/drizzle/no-query-in-loop` | `ast-grep` advise: `await` in the body of a `for`, `for…of`, `for…in`, `while` or `do`, or in a function passed to `.map` / `.forEach` / `.flatMap` (as `typescript/prisma/no-query-in-loop`), of a value whose type is `drizzle-orm`'s `QueryPromise` (§15.1) |
| `typescript/drizzle/no-sql-raw` | `ast-grep` forbid: `sql.raw(…)` (or `<alias>.raw(…)` with `import { sql as <alias> } from "drizzle-orm"`) whose argument is a template with `${}` or a `+` concatenation, or a variable declared from one in an enclosing block or the module |

context.md: schema, `out` and `dialect` in `drizzle.config.*`; `drizzle-kit generate` after a schema change, its files committed together and never written by hand; applied migrations are not edited; `push` needs confirmation, `push --force` is refused; deletes and updates have a `where`; the `sql` tag binds values.

### typescript/trpc

| Rule | Targets |
|---|---|
| `typescript/trpc/input-validate` | `ast-grep` forbid: `.input(…)` whose argument is `z.any()`, `z.unknown()`, `z.custom()` / `z.custom<T>()` without a check, `z.record(…)` of `z.any()` / `z.unknown()`, `(v) => v as T` or `(v: T) => v` |
| `typescript/trpc/trpc-error` | `ast-grep` forbid: `throw new Error(…)` or `throw Error(…)` inside a function passed as the first argument of `.query()`, `.mutation()` or `.subscription()`, in a file importing `@trpc/server` or when that function destructures `ctx` or `input`; not inside a `try` block |

context.md: `.input(<schema>)` and what is rejected; `TRPCError` with a `code` in procedures.

### typescript/hono

| Rule | Targets |
|---|---|
| `typescript/hono/validate-input` | `ast-grep` forbid, in files importing `hono`, `hono/*` or `@hono/*`: `.json()`, `.parseBody()`, `.formData()`, `.query()`, `.queries()` on `$C.req` or on a `req` destructured from the handler's parameter (`({ req })`, `({ req: r })`), unless the call (or its `await`) is passed straight to `X.parse` / `safeParse` / `parseAsync` / `safeParseAsync` (not on `JSON`, `Date`, …) or as the second argument of `parse(Schema, …)` / `v.parse(…)`; `c.req.param()` is not checked |
| `typescript/hono/cors-credentials-wildcard` | deprecated, replaced by `typescript/cors-credentials-wildcard` (which anchors `cors` by its declaration and covers `cors`, `@fastify/cors` and Nest) |
| `typescript/hono/no-raw-interpolation` | `ast-grep` forbid: a call of `raw` imported from `hono/html` (aliases included) whose argument is a template with `${}`, a `+` concatenation or a variable declared from one (the `drizzle/no-sql-raw` matcher) |

context.md: `zValidator` / `validator()` middleware and `c.req.valid()`; which raw reads are rejected; `cors()` with credentials names its origins; `raw()` is not built from templates.

### typescript/react-router

Detected by `@react-router/dev >= 7` (framework mode).

| Rule | Targets |
|---|---|
| `typescript/react-router/route-types` | `ast-grep` advise in `app/routes/**`, `app/root.tsx`: `LoaderFunctionArgs`, `ActionFunctionArgs`, `ClientLoaderFunctionArgs`, `ClientActionFunctionArgs`, `MetaFunction`, `LinksFunction`, `HeadersFunction` imported from `react-router` or `@remix-run/*` (the generated `Route.*` types instead) |

Whether an action validates its input and checks the caller cannot be decided from syntax (rules that guessed it from helper names were removed).

context.md: route module `loader` / `action` with `Route.*Args` types; actions are public endpoints (guidance, not checked); `data()` and `redirect()`.

### typescript/tanstack-router

Detected by `@tanstack/react-router` or `@tanstack/solid-router` (npm).

| Rule | Targets |
|---|---|
| `typescript/tanstack-router/no-generated-edits` | `permission` deny: `Edit(**/routeTree.gen.ts)` (the default `generatedRouteTree`) |

context.md: `routeTree.gen.ts` is generated from the routes directory and never edited by hand.

### typescript/vue

Detected by `vue >= 3`. Biome lints `.vue` templates with `html.experimentalFullSupportEnabled` (every snippet sets it); `eslint-plugin-vue` is not audited.

| Rule | Targets |
|---|---|
| `typescript/vue/typecheck` | `check` typecheck: `forge_exec_tool vue-tsc --noEmit` (`--build` when `tsconfig.json` has `references`); skipped while a Nuxt `tsconfig.json` points at a missing `.nuxt/`. A tool whose name has a `-` gets no wrapper function (not a POSIX function name) and is run as `forge_exec_tool <name>` |
| `typescript/vue/v-for-key` | deprecated, replaced by `typescript/vue/valid-templates` (`tool` biome `lint/correctness/useVueVForKey`) |
| `typescript/vue/valid-templates` | `tool` biome in `.vue` files: `lint/correctness/useVueVForKey`, `noVueVIfWithVFor`, `useVueValidVBind`, `useVueValidVOn`, `useVueValidVIf`, `useVueValidVElse`, `useVueValidVElseIf`, `useVueValidVHtml`, `useVueValidVText`, `useVueValidTemplateRoot`, `lint/nursery/useVueValidVFor` (snippet: error) |
| `typescript/vue/ref-value` | `tool` biome `lint/nursery/noVueRefAsOperand` in `.vue` and `.ts` files: a ref as an operand, a condition or a template-literal value |
| `typescript/vue/no-setup-props-destructure` | `tool` biome `lint/correctness/noVueSetupPropsReactivityLoss`: `setup({ a })` (destructuring in the body is not detected) |
| `typescript/vue/no-v-html` | `grep` forbid in `**/*.vue`: `v-html=`, `:innerHTML=`, `v-bind:innerHTML=` |
| `typescript/vue/no-vue2-template-syntax` | `grep` forbid in `**/*.vue`: `:x.sync=` / `v-bind:x.sync=`, `slot-scope=`, a `.native` modifier on `@x` / `v-on:x`, `@keyup.<digits>` (and `keydown`, `keypress`) |

context.md: `<script setup lang="ts">`, `vue-tsc` for `.vue` types, valid templates (`:key` with `v-for`, no `v-if` with `v-for`), `.value` on refs, props not destructured in `setup()`, no `v-html`, no Vue 2 template syntax.

### typescript/vue/nuxt

| Rule | Targets |
|---|---|
| `typescript/vue/nuxt/server-validate` | `ast-grep` forbid in `server/**`: `readBody(…)`, `readFormData(…)`, `readMultipartFormData(…)`, `getQuery(…)`, `getRouterParams(…)`, `getRouterParam(…)` (h3's `readValidatedBody`, `getValidatedQuery`, `getValidatedRouterParams` instead), unless the call (or its `await`) is passed straight to `X.parse` / `safeParse` / `parseAsync` / `safeParseAsync` (not on `JSON`, `Date`, `Number`, `URL`) |
| `typescript/vue/nuxt/no-public-secret` | `ast-grep` forbid in `nuxt.config.*`: a key under `runtimeConfig.public` whose name contains `secret`, `private`, `password` / `passwd`, `credential`, `token`, `api_key` / `apiKey`, `access_key`, `auth_key`, `service_role`, `dsn` or `database_url` (case-insensitive), unless it also says `publishable`, `public`, `anon` or `site_key` |
| `typescript/vue/nuxt/no-process-flags` | `grep` forbid in `**/*.vue`: `process.client`, `process.server`, `process.browser`, `process.dev` (`.ts` files: Nuxt marks them `@deprecated`) |
| `typescript/vue/nuxt/no-shared-module-state` | `ast-grep` forbid in `composables/**`, `app/composables/**`, `utils/**`, `app/utils/**`: `ref(…)`, `reactive(…)`, `shallowRef(…)`, `shallowReactive(…)` as the value of a top-level (or exported) `const` / `let` |

context.md: validated h3 readers in server routes; `runtimeConfig.public` reaches the browser; `vue-tsc` after `nuxt prepare`; `import.meta.client` in `.vue` files; no module-level reactive state in composables.

### typescript/vue/pinia

| Rule | Targets |
|---|---|
| `typescript/vue/pinia/store-to-refs` | `ast-grep` advise, `where` the called function is `pinia` `StoreDefinition`: `const { … } = useStore()` (destructure `storeToRefs(useStore())` instead) |

context.md: state and getters are destructured through `storeToRefs()`.

### typescript/svelte

Detected by `svelte >= 5`. Biome lints `.svelte` templates with `html.experimentalFullSupportEnabled` (the snippets set it); the syntax rules are `grep` targets.

| Rule | Targets |
|---|---|
| `typescript/svelte/runes-only` | `grep` forbid in `**/*.svelte`: `export let`, `$:` statements, `on:<event>` directives, `<slot`, `$$slots` / `$$props` / `$$restProps`, `createEventDispatcher`, `beforeUpdate` / `afterUpdate`, imports of `svelte/legacy` (in `.ts` files the deprecated APIs are `@deprecated` in Svelte's types) |
| `typescript/svelte/no-at-html` | `grep` forbid in `**/*.svelte`: `{@html ` |
| `typescript/svelte/each-key` | `tool` biome `lint/nursery/useSvelteRequireEachKey`: `{#each}` with a key |
| `typescript/svelte/no-debug-tags` | `tool` biome `lint/nursery/noSvelteAtDebugTags`: no `{@debug}` |

context.md: Svelte 5 runes, event attributes and snippets; legacy syntax and `{@html}` are rejected; keyed `{#each}`, no `{@debug}`.

### typescript/svelte/kit

| Rule | Targets |
|---|---|
| `typescript/svelte/kit/no-csrf-off` | `ast-grep` forbid in `svelte.config.*`, under `csrf`: `checkOrigin: false`, a `"*"` in `trustedOrigins` |
| `typescript/svelte/kit/no-app-stores` | `grep` forbid in `.svelte`, `.ts`, `.js`, `.mts`, `.mjs` files, `when` `@sveltejs/kit >= 2.12`: an import (static or dynamic) of `$app/stores` |
| `typescript/svelte/kit/no-env-modules` | `grep` forbid in the same files, `when` `@sveltejs/kit >= 3`: an import of `$env/static\|dynamic/private\|public` |
| `typescript/svelte/kit/external-redirect` | `ast-grep` forbid, `when` `@sveltejs/kit >= 3`: a call declared as `@sveltejs/kit`'s `redirect` (§15.6) whose second argument is a string or template starting with `http://` / `https://` and that has no third argument |

### typescript/angular

Detected by `@angular/core >= 20`.

| Rule | Targets |
|---|---|
| `typescript/angular/control-flow` | `grep` forbid in `**/*.html`, `**/*.ts` (inline templates): `*ngIf=`, `*ngFor=`, `*ngSwitchCase=`, `*ngSwitchDefault=`, `[ngSwitch]=` |
| `typescript/angular/no-bypass-sanitizer` | `ast-grep` forbid, `where` the receiver is `@angular/platform-browser` `DomSanitizer`: `bypassSecurityTrustHtml`, `…Script`, `…Style`, `…Url`, `…ResourceUrl` calls |
| `typescript/angular/typecheck` | `check` typecheck: `ngc -p <tsconfig> --noEmit` for each file of role `@angular-tsconfig` (§15.4); the role naming no file prints `skipped`; on changes to `**/*.ts`, `**/*.html` |

context.md: built-in control flow instead of `*ngIf` / `*ngFor` / `[ngSwitch]`; no `bypassSecurityTrust*`.

### typescript/solid

Detected by `solid-js` (npm).

| Rule | Targets |
|---|---|
| `typescript/solid/no-destructured-props` | `tool` biome `lint/correctness/noSolidDestructuredProps` (tsx) |
| `typescript/solid/for-component` | `tool` biome `lint/performance/useSolidForComponent` (tsx) |

context.md: props read as `props.x` or through `splitProps`, lists with `<For>`.

### typescript/tailwind

Detected by `tailwindcss >= 4`.

| Rule | Targets |
|---|---|
| `typescript/tailwind/no-removed-utilities` | `grep` forbid in `**/*.{tsx,jsx,vue,svelte,astro,html,css}`: `bg-`, `text-`, `border-`, `divide-`, `ring-`, `placeholder-opacity-<n>`, `flex-shrink(-<n>)`, `flex-grow(-<n>)`, `overflow-ellipsis`, `decoration-slice`, `decoration-clone` as a whole class (not a CSS property: `flex-grow:`) |
| `typescript/tailwind/config-referenced` | `check` lint, `when_changed`: `tailwind.config.*`, `**/*.css`: a root `tailwind.config.{js,ts,mjs,cjs}` needs a line starting with `@config` in a CSS file (tracked or untracked, not ignored) |
| `typescript/tailwind/apply-reference` | `check` lint on `.vue`, `.svelte`, `.astro` and `*.module.css` files: one with `@apply` has `@reference` or `@import "tailwindcss"`; each `@apply` line is reported (`path:line`, so only added lines count) |

context.md: CSS-first configuration, `@config` for a JS config, the removed utilities and their replacements, `@reference` with `@apply` in component styles.

### typescript/openapi

Detected by `openapi-typescript`, `orval` or `@hey-api/openapi-ts`. Both checks run only outside `--all`, and only when a tracked `package.json` has a script that runs one of the generators. An OpenAPI document is a changed `.json` / `.yaml` / `.yml` file with an `openapi:` or `swagger:` version in its first 4 KiB; a generated file is one whose first 15 lines say `auto-generated by openapi-typescript`, `auto-generated by @hey-api/openapi-ts` or `Generated by orval` (other generators' "do not edit" headers do not count).

| Rule | Targets |
|---|---|
| `typescript/openapi/regenerate` | `check` lint: `forge_openapi_sync regenerate`: a changed OpenAPI document and no changed generated file fail |
| `typescript/openapi/no-generated-edits` | `check` lint: `forge_openapi_sync hand-edits`: a changed generated file fails when no OpenAPI document, `orval.config.*`, `openapi-ts.config.*` or `redocly.*` changed, nor a `package.json` line naming the generator (its script or dependency version), and the repository tracks an OpenAPI document |

context.md: the client comes from the document through a generator script; the two change together, generated files are never edited by hand.

### typescript/nestjs

Detected by `@nestjs/core` or `@nestjs/common`. Decorators are matched as the bindings imported from their package (`import { Body as B } from "@nestjs/common"` included), never by a name alone. `tool_off`: Biome's `lint/style/useImportType` (§5.1), whose fix turns a DTO import into `import type`.

| Rule | Targets |
|---|---|
| `typescript/nestjs/validation-pipe-whitelist` | `ast-grep` forbid: `new $N(…)` without `whitelist: true` in its object argument, where `N` is `@nestjs/common`'s `ValidationPipe` |
| `typescript/nestjs/validation-pipe-forbid-non-whitelisted` | `ast-grep` advise: the same without `forbidNonWhitelisted: true` |
| `typescript/nestjs/dto-is-class` | `ast-grep` forbid: a parameter decorated `@Body()` / `@Query()` / `@Param()` (from `@nestjs/common`, no arguments) with no type, `any`, `unknown`, `object`, an object type literal or `Record<…>` |
| `typescript/nestjs/dto-not-type-import` | `ast-grep` forbid: a parameter decorated by `@nestjs/common`'s `Body` / `Query` / `Param` whose type is a name bound by a type-only import (`import type { X }`, `import { type X }`, `import type X`, aliases included): `design:paramtypes` loses the class and `ValidationPipe` validates nothing |
| `typescript/nestjs/validate-nested-type` | `ast-grep` forbid: a property decorated with class-validator's `ValidateNested` and no class-transformer `Type` decorator |
| `typescript/nestjs/http-exception` | `ast-grep` advise: `throw new Error(…)` in a method of a class decorated with `@nestjs/common`'s `Controller`, not inside a `try` block |
| `typescript/nestjs/express-path-syntax` | `ast-grep` advise, `when` `@nestjs/platform-express >= 11`: a string argument (or one in an array) with an unnamed `*` or `?`, `+`, `!`, `(`, `)`, `[`, `]` in a decorator call declared as `@nestjs/common`'s `Get`, `Post`, `Put`, `Patch`, `Delete`, `Options`, `Head` or `All` (§15.6), or in `$C.forRoutes(…)` where `C` is a `MiddlewareConfigProxy` (Nest converts the old syntax with a warning) |

context.md: `ValidationPipe` with `whitelist` and `forbidNonWhitelisted`; DTO classes on `@Body()` / `@Query()` / `@Param()`, imported as values; `@ValidateNested()` with `@Type()`; `HttpException` subclasses in controllers; path-to-regexp 8 route paths on the Express platform.

### typescript/kysely

| Rule | Targets |
|---|---|
| `typescript/kysely/no-sql-raw` | `ast-grep` forbid: a call `$CALL` of `<x>.raw(…)` / `<x>.lit(…)` whose argument is a template with `${}`, a `+` concatenation or a variable declared from one (the `drizzle/no-sql-raw` matcher), where `CALL` is Kysely's `RawBuilder` |
| `typescript/kysely/write-with-where` | `ast-grep` advise: `$DB.deleteFrom(…)` / `$DB.updateTable(…)` whose method chain reaches `execute()` / `executeTakeFirst()` / `executeTakeFirstOrThrow()` with no `where…` on the way, where `DB` is Kysely's `Kysely` or `Transaction` |
| `typescript/kysely/no-query-in-loop` | `ast-grep` advise: `await` in the body of a `for`, `for…of`, `for…in`, `while` or `do`, or in a function passed to `.map` / `.forEach` / `.flatMap` (as `typescript/prisma/no-query-in-loop`), of `$B.execute()` / `executeTakeFirst()` / `executeTakeFirstOrThrow()` where `B` is typed `kysely` (§15.1) |

context.md: the `sql` tag binds values, `sql.ref()` / `sql.id()` for names, no `sql.raw()` / `sql.lit()` from a template; deletes and updates have a `where`.

### typescript/typeorm

| Rule | Targets |
|---|---|
| `typescript/typeorm/no-synchronize` | `ast-grep` forbid: a pair `synchronize: true` / `dropSchema: true` in the object argument of `new $D(…)` where `D` is TypeORM's `DataSource`, of `TypeOrmModule.forRoot(…)`, or in an object a `TypeOrmModule.forRootAsync` `useFactory` returns (`TypeOrmModule` the binding imported from `@nestjs/typeorm`) |
| `typescript/typeorm/synchronize-expression` | `ast-grep` advise: the same keys set from an expression (not `true` / `false`) |
| `typescript/typeorm/no-raw-query-interpolation` | `ast-grep` forbid: `$R.query(…)` whose first argument is a template with `${}`, a `+` concatenation or a variable declared from one, where `R` is `DataSource`, `EntityManager`, `QueryRunner` or `Repository` |
| `typescript/typeorm/no-qb-where-interpolation` | `ast-grep` forbid: `where` / `andWhere` / `orWhere` / `having` / `andHaving` / `orHaving` / `orderBy` / `addOrderBy` with such a first argument on a `SelectQueryBuilder`, `UpdateQueryBuilder`, `DeleteQueryBuilder`, `SoftDeleteQueryBuilder` or `WhereExpressionBuilder` |
| `typescript/typeorm/no-query-in-loop` | `ast-grep` advise: `await` in the body of a `for`, `for…of`, `for…in`, `while` or `do`, or in a function passed to `.map` / `.forEach` / `.flatMap` (as `typescript/prisma/no-query-in-loop`), of `$R.$M(…)` where `R` is TypeORM's `Repository` or `EntityManager` (§15.1) |
| `typescript/typeorm/schema-sync-guard` | `guard` deny: `typeorm` / `typeorm-ts-node-esm` / `typeorm-ts-node-commonjs` `schema:sync`, `schema:drop` |
| `typescript/typeorm/migration-revert` | `guard` ask: the same bins with `migration:revert` |
| `typescript/typeorm/migration-present` | `check` migration, local database only: `forge_typeorm_pending @typeorm-datasource @esm-modules`: the data source (role `@typeorm-datasource`, no default; unset: `skipped`) runs `migration:run`, then `migration:generate --check` fails on "Unexpected changes in database schema"; a TypeScript data source through `typeorm-ts-node-esm` (an `@esm-modules` file) or `typeorm-ts-node-commonjs`; another failure prints `skipped:` |

context.md: migrations, never `synchronize` / `dropSchema`; parameters in `.query()` and in query builder conditions.

### typescript/mongoose

| Rule | Targets |
|---|---|
| `typescript/mongoose/no-request-filter` | `ast-grep` forbid: `$M.<op>($R.body \| query \| params, …)` or as the second argument, `<op>` one of `find`, `findOne`, `findOneAndUpdate`, `findOneAndDelete`, `findOneAndReplace`, `updateOne`, `updateMany`, `deleteOne`, `deleteMany`, `countDocuments`, `replaceOne`, where `M` is a Mongoose `Model` or `Query` and `R` an Express `Request` or a `FastifyRequest` |
| `typescript/mongoose/no-where-operator` | `ast-grep` forbid: a `$where` key in the arguments of a call on `$M`, or `$M.$where(…)`, where `M` is a Mongoose `Model` or `Query` |
| `typescript/mongoose/update-pipeline` | `ast-grep` advise, `when` `mongoose >= 9`: `$M.updateOne\|updateMany\|findOneAndUpdate\|findByIdAndUpdate(f, [<array>], …)` without an object argument holding `updatePipeline: true`, where `M` is a Mongoose `Model` or `Query` (advice: `mongoose.set("updatePipeline", true)` may be set elsewhere) |
| `typescript/mongoose/no-query-in-loop` | `ast-grep` advise: `await` in the body of a `for`, `for…of`, `for…in`, `while` or `do`, or in a function passed to `.map` / `.forEach` / `.flatMap` (as `typescript/prisma/no-query-in-loop`), of a value whose type is Mongoose's `Query` or `Aggregate` (§15.1: `find()`'s `QueryWithHelpers` is an alias of an intersection with `Query`) |

context.md: request input validated or `sanitizeFilter()`ed before a filter; no `$where`; update pipelines pass `updatePipeline: true`.

### typescript/temporal

| Rule | Targets |
|---|---|
| `typescript/temporal/workflow-determinism` | `ast-grep` forbid, in a file with a value import of `@temporalio/workflow`: a value import or `require()` of `node:*`, `fs`, `fs/promises`, `path`, `os`, `crypto`, `http`, `https`, `net`, `tls`, `dns`, `child_process`, `worker_threads`, `cluster`, `@temporalio/activity`, `@temporalio/client` or `@temporalio/worker`; `crypto.randomUUID()` / `crypto.getRandomValues()`. A project module holding activities imported by value is not detected (which module holds activities is not known without guessing) |

context.md: the workflow sandbox; activities through `proxyActivities` with `import type`; `uuid4()`.

### typescript/stripe

| Rule | Targets |
|---|---|
| `typescript/stripe/webhook-raw-body` | `ast-grep` forbid: `$S.webhooks.constructEvent(…)` / `constructEventAsync(…)` whose first argument is `JSON.stringify(…)`, `await <x>.json()` or a variable declared from one in an enclosing block or the module, where `S` is Stripe's `Stripe` |

context.md: webhook payloads are the raw body.

### typescript/cloudflare-workers

Detected by `wrangler.toml`, `wrangler.json`, `wrangler.jsonc` or the `wrangler` dependency.


| Rule | Targets |
|---|---|
| `typescript/cloudflare-workers/no-ctx-destructure` | `ast-grep` forbid: `const { … } = $C` where `C: {type: {module: ["@cloudflare/workers-types", "@wrangler-types"], name: ExecutionContext}}` (the `wrangler types` output through role `@wrangler-types`, §15.4) |
| `typescript/cloudflare-workers/no-pass-through-on-exception` | `ast-grep` advise: `$C.passThroughOnException()`, `C` as above |
| `typescript/cloudflare-workers/env-types-generated` | `check` codegen (§10.1), `when` wrangler `>=4.153.0`: `wrangler types <file> --check` for each file of role `@wrangler-types` (the config hash the file records is the current config's); `when_changed: [@wrangler-config, @wrangler-types]` |
| `kubernetes/helm/chart-api-v2` | `grep` forbid on `Chart.yaml`: a top-level `apiVersion: v1` |

### typescript/cloudflare-workers/stripe

| Rule | Targets |
|---|---|
| `typescript/cloudflare-workers/stripe/constructevent-async` | `ast-grep` forbid: `$S.webhooks.constructEvent(…)`, where `S` is Stripe's `Stripe` (on Workers only `constructEventAsync` works) |

context.md: `await stripe.webhooks.constructEventAsync(…)` on Workers.

### typescript/mcp

Detected by `@modelcontextprotocol/sdk` or `@modelcontextprotocol/server`.

| Rule | Targets |
|---|---|
| `typescript/mcp/stdio-no-stdout` | `ast-grep` forbid, in a file importing `StdioServerTransport` from `@modelcontextprotocol/sdk/server/stdio.js` or `@modelcontextprotocol/server`: `console.log`, `console.info`, `console.debug`, `process.stdout.write` |

context.md: stdout is the JSON-RPC stream; logs on stderr.

### typescript/upstash-ratelimit

| Rule | Targets |
|---|---|
| `typescript/upstash-ratelimit/check-limit` | `ast-grep` forbid: an expression statement `$RL.limit(…)` or `await $RL.limit(…)` (result discarded), where `RL` is `@upstash/ratelimit`'s `Ratelimit` (the exported classes extend it) |

context.md: `limit()` never throws; check `success`.

### typescript/fastify

Detected by `fastify >= 5`. `Fastify()` returns an intersection (`FastifyInstance & SafePromiseLike<…>`), whose `FastifyInstance` member counts (§15.1), like plugin parameters and annotated instances.

| Rule | Targets |
|---|---|
| `typescript/fastify/plugin-style` | `ast-grep` forbid: an `async` arrow or function expression with a third parameter, passed first to `$F.register(…)` (where `F` is a `FastifyInstance`) or to fastify-plugin's default import |
| `typescript/fastify/full-json-schema` | `ast-grep` forbid: in the `schema` of `$F.route({…})` / `$F.<method>(url, {…}, …)` (where `F` is a `FastifyInstance`), a `body`, `querystring`, `params` or `headers` value, or a `response` status value, that is an object literal with properties, no spread and no JSON Schema keyword key |
| `typescript/fastify/no-permissive-trust-proxy` | `ast-grep` advise: `trustProxy: true` in the object passed to fastify's default or named `fastify` import (aliases included) |

context.md: the two plugin signatures; full JSON Schemas in routes; `trustProxy` is not `true`.

### typescript/elysia

| Rule | Targets |
|---|---|
| `typescript/elysia/body-schema` | `ast-grep` forbid: `$APP.post` / `put` / `patch(path, handler, …)` whose handler's first parameter destructures `body`, without a third argument object holding `body`, where `APP` is an `Elysia` |

context.md: a `body` schema on routes that read the body.

### typescript/apollo

| Rule | Targets |
|---|---|
| `typescript/apollo/csrf-prevention` | `ast-grep` forbid: `new $N({…, csrfPrevention: false})`, where `N` is `@apollo/server`'s `ApolloServer` |
| `typescript/apollo/no-stacktrace` | `ast-grep` forbid: the same with `includeStacktraceInErrorResponses: true` |

context.md: CSRF prevention on; no forced stack traces.

### typescript/graphql-yoga

| Rule | Targets |
|---|---|
| `typescript/graphql-yoga/masked-errors` | `ast-grep` forbid: a call of `createYoga` imported from `graphql-yoga` (aliases included) with `maskedErrors: false` |

context.md: error masking on; `GraphQLError` for client errors.

### typescript/graphql-codegen

Detected by `@graphql-codegen/cli`.

| Rule | Targets |
|---|---|
| `typescript/graphql-codegen/up-to-date` | `check` codegen (§10.1): `graphql-codegen --check` (exit 1 and the stale files when an output would change), on changes to `.graphql`, `.gql`, TypeScript and JavaScript files and the codegen config |

context.md: generated files are regenerated, never edited by hand.

### typescript/effect

`Effect` is the binding imported from `effect` (`import { Effect }`, aliases included) or the namespace import of `effect/Effect`. "In a generator" means the nearest enclosing function is the generator passed to `Effect.gen(…)`.

| Rule | Targets |
|---|---|
| `typescript/effect/no-run-inside-gen` | `ast-grep` forbid: `Effect.runPromise` / `runSync` / `runFork` (and `…Exit` / `…With`) called in an `Effect.gen` generator |
| `typescript/effect/no-throw-in-gen` | `ast-grep` forbid: a `throw` statement in an `Effect.gen` generator |

context.md: `yield*` inside generators, `run*` at the edge; `Effect.fail` with tagged errors.

### typescript/mikro-orm

Detected by `@mikro-orm/core`.

| Rule | Targets |
|---|---|
| `typescript/mikro-orm/no-global-context` | `ast-grep` forbid, outside test files (`*.test.*`, `*.spec.*`, `__tests__/`, `e2e/`): `allowGlobalContext: true` in the object passed to `MikroORM.init(…)`, `new MikroORM(…)` or `defineConfig(…)`, those names imported from `@mikro-orm/*` (aliases included) |
| `typescript/mikro-orm/schema-guard` | `guard` deny: `mikro-orm` / `mikro-orm-esm` `schema:drop` or `schema:fresh` with `--run` / `-r` |
| `typescript/mikro-orm/schema-update` | `guard` ask: `schema:update` with `--run` / `-r`, `migration:fresh` |
| `typescript/mikro-orm/migration-present` | `check` migration, `requires_local_db`: `mikro-orm migration:up`, then `mikro-orm migration:check` (exit 1 when the entities differ from the migrated schema) |

context.md: request contexts, never a global one; migrations; which schema commands are refused or asked.

### typescript/redis

Detected by `ioredis`, `redis` or `@redis/client`.

| Rule | Targets |
|---|---|
| `typescript/redis/no-keys` | `ast-grep` forbid: `$C.keys(…)` / `$C.KEYS(…)`, where `C` is ioredis's `Redis` or `Cluster`, or node-redis's `RedisClient` or `RedisCluster` (`createClient()`'s `RedisClientType`, an alias of an intersection with `RedisClient`, §15.1) |

context.md: `SCAN` iterators instead of `KEYS`.

### typescript/jsonwebtoken

`verify`, `sign` and `decode` are members of jsonwebtoken's default or namespace import, or its named imports (aliases included).

| Rule | Targets |
|---|---|
| `typescript/jsonwebtoken/verify-algorithms` | `ast-grep` forbid: `verify(…)` without a third argument that is an object with `algorithms` (or a variable / member holding options); `algorithms` containing `"none"` or `ignoreExpiration: true` in `verify`'s options; `algorithm: "none"` in `sign`'s options |
| `typescript/jsonwebtoken/no-decode-trust` | `ast-grep` advise: every `decode(…)` call (it does not verify) |

context.md: `verify` with an algorithm list, expiry kept; `decode` is not trust.

### typescript/better-auth

`betterAuth` is the binding imported from `better-auth` (aliases included).

| Rule | Targets |
|---|---|
| `typescript/better-auth/no-csrf-disable` | `ast-grep` forbid: `disableCSRFCheck: true` or `useSecureCookies: false` anywhere in `betterAuth(…)`'s arguments |
| `typescript/better-auth/trusted-origins` | `ast-grep` forbid: `trustedOrigins` in `betterAuth({…})` that is an array holding `"*"`, or a function whose returned array holds its first parameter's `.headers.get("origin")` (alone or with a `??` / `\|\|` fallback) |
| `typescript/better-auth/schema-check` | `check` migration: `forge_do better-auth schema-check <file>` (`better-auth check schema --config <file>`, from `auth` 1.7.6, the release that added it; an older release is reported not enforced by its adapter, the one gate) (the `auth` package's CLI) on the first file of role `@better-auth-config`: exit 1 (`SCHEMA_MISMATCH`) fails, exit 2 (cannot check) prints `skipped`; a config without the Drizzle or Prisma adapter checks the live database, so it runs only with a local one (§10.1 step 5) |

context.md: CSRF checks and Secure cookies on; real trusted origins.

### typescript/aws-lambda

Detected by `@types/aws-lambda` (or `aws-lambda`).

| Rule | Targets |
|---|---|
| `typescript/aws-lambda/no-callback-handler` | `ast-grep` forbid: a parameter annotated with `Callback` / `Callback<…>` imported from `aws-lambda` (aliases included); `$C.callbackWaitsForEmptyEventLoop`, where `C` is aws-lambda's `Context`. `context.succeed` / `fail` / `done` are `@deprecated`, left to the deprecation rules |

context.md: async handlers; callback handlers rejected.

### typescript/resend

| Rule | Targets |
|---|---|
| `typescript/resend/check-error` | `ast-grep` forbid: an expression statement `$R.emails.send(…)` / `$R.batch.send(…)`, awaited or not (result discarded), where `R` is Resend's `Resend` |

context.md: `{ data, error }` results are handled.

### typescript/pulumi

| Rule | Targets |
|---|---|
| `typescript/pulumi/protect-ratchet` | `grep` no-decrease: `protect: true` (whole words) |
| `typescript/pulumi/no-output-in-string` | `ast-grep` forbid: an expression in a `${…}` of an untagged template literal, or an operand of `+` with a string or template operand, whose type is `@pulumi/pulumi`'s `Output` / `OutputInstance` (§15.1: `Output<T>` is an alias of an intersection) |

context.md: protected resources stay protected.

### typescript/aws-cdk

Detected by `cdk.json` or the `aws-cdk-lib` dependency.

| Rule | Targets |
|---|---|
| `typescript/aws-cdk/removal-policy-destroy` | `ast-grep` advise: `$E.DESTROY`, where `E` is aws-cdk-lib's `RemovalPolicy`; `new $B(…, {autoDeleteObjects: true})`, where `B` is aws-cdk-lib's `Bucket` |
| `typescript/aws-cdk/context-committed` | `check` lint: with `cdk.json`, `cdk.context.json` is not git-ignored (`git check-ignore`), `when_changed: [.gitignore, **/.gitignore, cdk.json]` |
| `typescript/aws-cdk/nag-suppressions` | `ast-grep` no-increase: `NagSuppressions.add…Suppressions…(…)`, `NagSuppressions` imported from `cdk-nag` (aliases included) |

context.md: destroy policies are advice; `cdk.context.json` committed; cdk-nag findings fixed, not suppressed.

### typescript/express

| Rule | Targets |
|---|---|
| `typescript/express/session-cookie-flags` | `ast-grep` forbid: `secure: false` / `httpOnly: false` in the `cookie` object of a call of express-session's default import, or in the object passed to cookie-session's default import |
| `typescript/express/no-permissive-trust-proxy` | `ast-grep` advise: `$A.set("trust proxy", true)`, where `A` is an Express `Application` (`express-serve-static-core`) |
| `typescript/express/path-syntax-v5` | `ast-grep` forbid, `when` `express >= 5`: a first argument of `$R.get\|post\|put\|patch\|delete\|options\|head\|all\|use\|route(…)` (a string, or a string in an array) with an unnamed `*` or `?`, `+`, `!`, `(`, `)`, `[`, `]` (path-to-regexp 8 throws), where `R` is an Express `IRouter` (`express-serve-static-core`: applications and routers) |
| `typescript/express/no-req-query-assign` | `ast-grep` forbid, `when` `express >= 5`: an assignment (or `+=`, …) to `$R.query`, `$R.query.<p>` or `$R.query[<k>]`, where `R` is an Express `Request` (a getter in Express 5) |
| `typescript/express/no-async-errors` | `ast-grep` forbid, `when` `express >= 5`: an import or `require` of `express-async-errors` |
| `typescript/express/no-async-handler` | `ast-grep` advise, `when` `express >= 5`: an import or `require` of `express-async-handler` or `express-promise-router` (redundant: Express 5 handles rejected promises) |
| `typescript/express/no-v4-response-signatures` | `ast-grep` forbid, `when` `express >= 5`: `$R.send(<number>)` (one argument) and `$R.redirect("back")`, where `R` is an Express `Response` |
| `typescript/express/types-match` | `check` lint: `forge_dep_major_match express @types/express @types/express-serve-static-core`: in each changed `package.json` declaring both, the lowest major of the types package's range equals express's (`^4.17.21` on `^5.1.0` fails); a spec that is not a range (`catalog:`, `workspace:`, `*`) is reported as not compared; `when_changed: [**/package.json]` |

context.md: no literal `secure: false` / `httpOnly: false` session cookies; `trust proxy` is not `true`; Express 5 path syntax, `req.query` read-only, no async wrappers, no Express 4 `res.send(<number>)` / `res.redirect("back")`.

### typescript/bullmq

| Rule | Targets |
|---|---|
| `typescript/bullmq/worker-connection` | `ast-grep` forbid: `connection: new $R(…)` without `maxRetriesPerRequest: null` in the options of `new $W(…)`, where `W` is BullMQ's `Worker` and `R` ioredis's `Redis` (a connection in a variable is not followed) |

context.md: `maxRetriesPerRequest: null` on worker connections.

### typescript/bcrypt

Detected by `bcrypt` or `bcryptjs`.

| Rule | Targets |
|---|---|
| `typescript/bcrypt/no-sync-hash-in-async` | `ast-grep` forbid: `hashSync` / `compareSync` / `genSaltSync` of bcrypt's or bcryptjs's default, namespace or named import (aliases included), when the nearest enclosing function is `async` |

context.md: await the async variants in async code.

### typescript/aws-s3

Detected by `@aws-sdk/client-s3` or `@aws-sdk/s3-presigned-post`.

| Rule | Targets |
|---|---|
| `typescript/aws-s3/no-public-acl` | `ast-grep` advise: the string `public-read` / `public-read-write` as the `ACL` of `new $C({…})`, where `C` is `PutObjectCommand`, `CreateMultipartUploadCommand` or `CopyObjectCommand` (`@aws-sdk/client-s3`), or anywhere in the arguments of `createPresignedPost` imported from `@aws-sdk/s3-presigned-post` |

context.md: no public ACLs on uploads.

### typescript/anthropic

Detected by `@anthropic-ai/sdk` (npm).

| Rule | Targets |
|---|---|
| `typescript/anthropic/no-budget-tokens` | `ast-grep` forbid: `$C.messages.$M({…})` / `$C.beta.messages.$M({…})` (`create`, `stream`, `parse`, `countTokens`), `$C` typed `@anthropic-ai/sdk` `Anthropic` / `BaseAnthropic`, whose options have a `model` string literal from the data file's `rejects budget_tokens` rows (`ai-models`) and a `thinking` object with a `budget_tokens` key |
| `typescript/anthropic/no-forced-tool-choice` | `ast-grep` forbid: the same calls with a `model` literal from the `rejects forced_tool_choice` rows and `tool_choice: {type: 'any' \| 'tool'}` |

### typescript/langchain

Detected by `langchain`, `@langchain/core` or `@langchain/classic`.

| Rule | Targets |
|---|---|
| `typescript/langchain/no-legacy-chains` | `ast-grep` advise: importing `LLMChain`, `ConversationChain`, `RetrievalQAChain`, `ConversationalRetrievalQAChain`, `initializeAgentExecutorWithOptions` or `AgentExecutor` from `@langchain/classic…` or `langchain/chains…` / `langchain/agents…` |

context.md: `createAgent`; the legacy chains live in `@langchain/classic`.

### typescript/ai

Detected by `ai` (the Vercel AI SDK).

| Rule | Targets |
|---|---|
| `typescript/ai/no-system-in-messages` | `ast-grep` forbid, `when` `ai >= 7`: a call declared as `ai`'s `generateText` or `streamText` (§15.6) whose options object has a `messages` or `prompt` array literal holding an object with `role: "system"`, and no `allowSystemInMessages: true` |

context.md: system prompts go in `instructions`.

### typescript/opentelemetry

Detected by `@opentelemetry/api` or `@opentelemetry/sdk-node`.

| Rule | Targets |
|---|---|
| `typescript/opentelemetry/init-before-imports` | `ast-grep` advise: in a file with a value import of `@opentelemetry/sdk-node`, a value import of a library the Node auto-instrumentations patch (`http`, `https`, `net`, `dns` and their `node:` forms, `express`, `fastify`, `koa`, `pg`, `mysql2`, `mongodb`, `mongoose`, `ioredis`, `redis`, `graphql`, `kafkajs`, `undici`, `pino`, `winston`, `@aws-sdk/*`, …) |
| `typescript/opentelemetry/semconv-stable-keys` | `ast-grep` forbid: `$S.setAttribute(<key>, …)` / `$S.setAttributes({<key>: …})` with an old key (`http.method`, `http.status_code`, `http.url`, `http.target`, `http.scheme`, `http.user_agent`, `net.peer.name` / `port`, `net.host.name` / `port`, `db.statement`, `db.system`), where `S` is `@opentelemetry/api`'s `Span` |

context.md: the SDK in its own file loaded first; stable semantic convention keys.
