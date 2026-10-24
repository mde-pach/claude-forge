# TypeScript repository environments (better-auth, zod, inngest-js)

Owner of this section: TS repo environments. Files: `repos/{better-auth,zod,inngest-js}.yaml`,
`images/{better-auth,zod,inngest-js}.Dockerfile`. Generic machinery (containers, JUnit parsing, Biome JSON
parsing, warm-up) is `repo_env.py` / `warm_cache.py`, unchanged. Measured 2026-10-24.

## Images

Each image is `FROM forge-exp/base:v1` and adds only the pnpm versions that the candidate commits name in
`packageManager`, so that corepack never has to download them (trials are offline). The base default (pnpm
10.34.7, `lastKnownGood.json`) is restored afterwards. They also set `COREPACK_ENABLE_NETWORK=0`, so a missing
version fails loudly instead of hanging, and `npm_config_update_notifier=false`. No dependencies are baked in.

| Image | Extra pnpm versions (corepack, `/opt/corepack/v1/pnpm`) | Why |
|---|---|---|
| `forge-exp/better-auth:v1` | 10.30.2, 11.0.3, 11.1.1 | candidate parents use these three |
| `forge-exp/zod:v1` | 10.12.1 | all pnpm candidates |
| `forge-exp/inngest-js:v1` | 10.12.1+sha512.f0dda858… | exact spec with integrity suffix, as in `package.json` |

Each image adds about 20 to 60 MB over the base. Build (from `experiment/`):
`DOCKER_BUILDKIT=1 docker build -f harness/images/<id>.Dockerfile --secret id=ca,src=/root/.ccr/ca-bundle.crt -t forge-exp/<id>:v1 harness/images`.
**Rebuild them whenever `forge-exp/base:v1` is rebuilt.** They were last rebuilt on the base image `a23b084baf2c`.

## pnpm store: read-only cache, writable view (all three repos)

The host store `/work/cache/pnpm` is mounted read-only at `/cache/pnpm`. It holds `v10/` (pnpm 10: zod, inngest-js,
better-auth at pnpm 10.30.2) and `v11/` (pnpm 11: better-auth).
- **pnpm 11 cannot use a read-only store.** It opens its SQLite index `v11/index.db` read-write and fails with
  `ERR_SQLITE_ERROR unable to open database file`. This was verified.
- **Fix, in `install`:** build a writable store view in `/tmp/pnpm-store/<v>/`. The content directory `files/`
  is a symlink to the read-only cache. Everything else (`index.db`, `index/`, `projects/`; tens of MB) is copied.
  Then `pnpm install --frozen-lockfile --offline --store-dir /tmp/pnpm-store --package-import-method copy`.
  Offline installs never write into `files/`.
- **Copy, not hardlink:** hardlinks across bind mounts fail anyway (EXDEV), so `copy` is explicit. Every
  workspace gets a full copy of `node_modules`: 2.8 GB for better-auth, about 1.3 GB for zod and inngest-js.
- `env` sets `pnpm_config_store_dir` / `npm_config_store_dir` (pnpm 11 reads `pnpm_config_*`, pnpm 10 reads
  `npm_config_*`) and `package_import_method=copy`. A later `pnpm install` by the agent therefore uses the same
  store view.
- The warm-up (`warm`, network, store rw) runs the same install against `/cache/pnpm` directly. This includes
  pnpm's side-effects cache for allowed builds (better-sqlite3, prisma engines, esbuild…), so these builds are
  restored offline without network or a compiler.

## Git hooks (all three)

The repositories' root `prepare` scripts install git hooks: lefthook in better-auth, husky in zod.
- **Why that is a problem:** the hooks land in `.git/hooks` / `core.hooksPath`. The pre-commit hook is a
  Forge-owned path in the parity check (DESIGN §7), so the baseline arm would fail parity (its Forge-owned set
  must be empty). A hook manager would also shadow Forge's own hook.
- **What install does:** it runs husky with `HUSKY=0` (`LEFTHOOK=0` does *not* stop `lefthook install`, verified).
  Then it deletes every non-sample file in `.git/hooks` and unsets `core.hooksPath`.
- Same in both arms. **Judgement call:** this removes hook managers that a real clone would install. The
  conservative choice is to keep the parity check meaningful and leave Forge's hook unshadowed.

## better-auth

Workspace: `packages/**`, `docs`, `e2e/**`, `test` (44 projects), pnpm 11.1.1 / 11.0.3 / 10.30.2, vitest 4.1
root `projects` config, Biome 2.4.

**A build is required.**
- Packages import each other through package exports that point at `dist/`. The root config's
  `ssr.resolve.conditions: ["dev-source"]` is not inherited by the vitest projects.
- Without a build, `vitest run packages/electron/test/electron.test.ts` fails with
  `Cannot find package '@better-auth/core/api'` (verified).
- CI (`ci.yml`) and CONTRIBUTING both do `pnpm install`, then `pnpm build`, then vitest from the root.
- `install`: the offline install, then `turbo run build --filter='./packages/*'` (= `pnpm build`).

**`test_cmd` rebuilds before testing, as CI's `turbo test` does** (`dependsOn: ["^build","build"]`).
- For each target, the nearest directory with a `package.json` is its package. `turbo run build
  --filter=./<pkg>...` rebuilds those packages plus their workspace dependencies. Turbo's local cache gives
  hits for unchanged packages: 0.4 s when nothing changed, about 14 s when packages/better-auth changed.
- Then `vitest run {targets}` runs from the root with `--reporter=default --reporter=junit`.
- **Why:** without the rebuild, tests that import another package would run against a stale `dist/` built at
  install time. An agent fix in packages/core would be invisible to packages/better-auth tests. Validation at
  C would also differ from scoring of an agent tree that is identical to C.
- **If the build fails,** the command prints `[forge-exp] workspace build failed; tests not run` and exits 3 with
  no JUnit, so every requested id is "not run" (= error). A build failure caused by the agent's code is a fail.
  classify.py may grep that marker.

**Determinism (parity).**
- The turbo cache lives outside the tree (`TURBO_CACHE_DIR=/tmp/turbo-cache`). Turbo's per-package
  `.turbo/turbo-build.log` contains timings and is deleted after install.
- After two independent installs + builds of the same commit, the fingerprints (harness/fingerprint.py) differed
  in exactly four paths:
  - `docs/.source/{browser,server}.ts` (fumadocs-mdx postinstall);
  - `~/.config/turborepo/telemetry.json` (random id);
  - `~/.cache/checkpoint-nodejs/signature` (Prisma checkpoint client).
- `dist/` was byte-identical across the two builds. All four paths are now removed at the end of install, and
  `CHECKPOINT_DISABLE=1` is set. The re-check result is at the end of this file.

**Timings** (container limits from repo_env: 2 CPUs, 6 GB):
- Offline install takes about 60 to 80 s for pnpm and 5 to 6 min for the full build. The whole `install` takes
  357 to 486 s (412 s in warm_cache's verify run).
- A test run of electron.test.ts takes 12 to 15 s (85 tests, about 9 to 12 s in vitest).

**Smoke** (first main candidate `d7c41baa`, fix(electron), `packages/electron/test/electron.test.ts`):
- At C: 85 passed.
- At C^ with C's test file: 1 failed, 84 passed. The failing test is
  `Electron > /electron/init-oauth-proxy should forward each Set-Cookie … as a separate header`, which is F2P.

**Warm-up:** 60 main + 10 pilot candidates, 107 commits, 24 distinct lockfile keys, 24/24 ok, about 2.5 to
5.5 min each with 2 jobs. Offline verify install @ 89545631 ok (412 s).

**Notes and open points.**
- Tests needing Postgres, MySQL, MSSQL, Mongo or Redis (adapter tests, e2e/adapter, redis-storage) cannot run
  offline. They will fail at both C^ and C and be dropped by validation, as intended. The default unit config
  of packages/better-auth already excludes `src/adapters/**`, and sqlite and in-memory tests run.
- Full install, as CI does, including `docs` (Next.js). `--filter '!docs'` would save about 0.5 GB per
  workspace (2.0 vs 2.5 GB `node_modules`), but it is not what contributors run, so it was not used.
- Node 22 (base) vs `.nvmrc` 24: CI's test matrix runs both 22.x and 24.x, so 22 is a supported runtime.

## zod

Workspace `packages/*`, pnpm 10.12.1, vitest 4.1 root `projects` config, Biome 1.9.

**No build.** The vitest config resolves `zod` through the `@zod/source` condition, i.e. from
`packages/zod/src`.
- `install`: the offline install with `HUSKY=0`, the git-hook cleanup, and `rm -rf packages/docs/.source`
  (non-deterministic fumadocs output, found by the same double-install fingerprint check; after the fix the
  fingerprints are identical, 612 entries).
- `test_cmd`: `vitest run {targets}` from the root, with the default and JUnit reporters (= `pnpm test`).

**Typecheck is kept** (`typecheck.enabled: true` in the repo config: tsc over `*.test.ts`, which contain
type-level assertions).
- Each test then appears twice in the JUnit output (runtime and type check, with the same classname and name).
- `repo_env.parse_junit` merges duplicates worst-wins, so a test passes only if both pass.
- The type check costs about 25 to 55 s per run (it type-checks the project once).

**Timings:**
- Offline install: 17 to 54 s (22 s typical when the page cache is warm).
- The two first-candidate test files take 29 to 64 s, of which typecheck is 25 to 54 s.

**Smoke** (first main candidate `e6c213ec`, `__proto__` keys in JSON-schema conversion, 2 test files):
- At C: 156 ids passed (312 vitest results incl. typecheck).
- At C^ with C's tests: 5 failed, 151 passed. All 5 are the new `__proto__` tests, which are F2P.

**Warm-up:** 60 main + 5 pilot (only 5 exist), 103 commits, 4 lockfile keys, 4/4 ok. Offline verify ok (76 s).

**Open:** the last 14 main candidates (index 102 onwards) switch to `packageManager: nub@0.8.3`, with workspaces
in `package.json` and no `pnpm-workspace.yaml`. This environment does not support them: corepack has no `nub`,
and pnpm without `pnpm-workspace.yaml` would not see the workspace. Only the first 10 accepted tasks are needed,
so these are unlikely to be reached. If they are, they will fail install and must be recorded as environment
drops, not task failures.

## inngest-js

Workspace `packages/*`, pnpm 10.12.1, per-package vitest 3.2 configs (no root vitest config), Biome 2.1.
`test_cmd` follows CI (`pr.yml` + `.github/actions/setup-and-build`): `pnpm install`, build `packages/test`, then
`npm run test` (= `vitest run`) inside `packages/inngest`.

**`install`:**
- It runs `pnpm install --frozen-lockfile --offline --ignore-scripts`. The root and eslint-plugin-internal
  `preinstall` is `npx only-allow pnpm`, which downloads `only-allow` (impossible offline). The only allowed
  dependency builds (`onlyBuiltDependencies`: aws-sdk, core-js, protobufjs) are notices or version checks.
  **Judgement call:** skipping them changes nothing that the tests use.
- Then `pnpm run build` in packages/test. The inngest tests import `@inngest/test`, whose `dist` is used, and
  that package depends on the registry `inngest@4.0.0`, not the workspace.
- Then `pnpm run pb:version` in packages/inngest, which generates the gitignored `src/version.ts` (CI does this
  through `prebuild`).
- packages/inngest's own build is not run: its unit tests import source, and `test:dist` is a separate CI job.
- Both fingerprints were identical (1167 entries).

**`test_cmd`:**
- Targets are grouped by their nearest `package.json`. vitest runs in each package directory with that
  package's config, writing one JUnit per package.
- A small python3 step merges the JUnit files into `{junit}`, prefixing classnames (and the load-error name)
  with the package path so that ids are repo-relative.
- This was verified with packages/inngest + packages/otel targets together (5 ids, all passed at `61dca106`).
- Typecheck is kept as in zod. In packages/inngest it costs about 40 s per run (tsc over `tsconfig.types.json`).

**`path_prepend`:** `/work/node_modules/.bin:/work/packages/inngest/node_modules/.bin`. The root has biome and
vitest; the main package adds `tsc`, `tsx` and the others (D5 dev shell). The root entries take precedence.

**Timings:**
- Offline install: 61 to 79 s.
- stream.test.ts: 42 to 46 s (typecheck about 40 s, runtime under 1 s).

**Smoke** (first main candidate `9b5ea64c`, streaming heartbeat crash, `packages/inngest/src/helpers/stream.test.ts`):
- At C: 2 ids passed (4 vitest results incl. typecheck).
- At C^ with C's test: 2 failed (`createStream > finalize does not reject after the consumer cancels the
  stream`, `createStream > heartbeat does not throw after the consumer cancels the stream`), which are F2P.

**Warm-up:** 49 main (all) + 10 pilot, 90 commits, 7 lockfile keys, 7/7 ok. Offline verify ok (69 s).

## Lint (all three)

`lint_cmd` keeps only the files that still exist. A deleted file would otherwise yield a biome
`internalError/io` "finding" at the end state. If no file is left, it prints `{"diagnostics":[]}` rather than
running biome on the whole repository. Otherwise it runs
`biome lint --reporter=json --no-errors-on-unmatched --files-ignore-unknown=true <files>` with the repository's
own config and Biome version (1.9 for zod, 2.1 for inngest-js, 2.4 for better-auth).
`repo_env.lint` already parses both JSON shapes:
- Biome 2: `location.path` is a string and `start.line` is given.
- Biome 1.9: `path.file` plus `span`, converted to a line.

Verified through `repo_env.lint` on zod (1.9): noDebugger, noUnusedImports, noVar and noUnusedVariables are
reported with the file and line. For better-auth (2.4), only biome's raw JSON output was checked (`noDebugger` with
`start.line`), not a call through `repo_env.lint`.

## Disk

- Host store after all three warm-ups: `/work/cache/pnpm` 9.4 GB (v10 4.9 GB, v11 4.6 GB), shared with no other
  repo.
- Per workspace (docker overlay): `node_modules` 2.8 GB plus about 0.2 GB of dist for better-auth, about 1.3 GB
  for zod and inngest-js.
- Removing a better-auth container takes up to a minute (hundreds of thousands of files).
- Free disk dropped to about 3.6 GB at one point while several agents were warming at once.
- Recommendation: do not run more than 2 better-auth containers at a time.

## better-auth determinism re-check (after the fixes)
Two more independent install + build runs of 89545631 had identical fingerprints (2804 entries, 0 differences).
Under host load from other agents' validation and trial containers, `install` took 675 and 746 s, against
357 to 486 s on a quieter host.
