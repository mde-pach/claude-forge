# Images, caches and repository environments

Owner: harness / repo environments. Contract: `INTERFACES.md`. Code: `repo_env.py`, `warm_cache.py`,
`images/base.Dockerfile`, `images/bin/uv-cache-overlay`, `repos/*.yaml`.

## forge-exp/base:v1 (shared by both arms)

| Item | Pin |
|---|---|
| Base | `debian:bookworm-slim@sha256:7ff6ba31527b9e336162fa061746c366c4b2032f4d2cff2078bea43f78efe3f8` |
| Image ID (local build, not pushed) | `sha256:a23b084baf2c9f031ec2cd5269b50034d1890e2a56135902fb8b7c39c0f67fc2` (1.53 GB on disk, 362 MB content) |
| uv | 0.12.24 (static binary, sha256 checked) |
| Python (uv-managed, `/opt/uv-python`) | cpython 3.12.15, 3.13.16 (system python3 3.11.2 also present) |
| Node | 22.23.3 (`/opt/node`, sha256 checked); corepack shims in `/opt/node/bin` |
| pnpm | 10.34.7 via `corepack install -g`, `COREPACK_HOME=/opt/corepack` |
| bun / bunx | 1.4.3 (sha256 checked) |
| apt | git, curl, ca-certificates, bubblewrap 0.8.0, socat 1.7.4.4, ripgrep 13, jq, procps, python3, xz-utils. No build-essential (no repo needed it). |
| User | `dev` 1000:1000, `HOME=/home/dev`, workdir `/work`, `CMD sleep infinity` |

Every pinned release is at least two weeks old (picked 2026-10-24): newer uv 0.13.x, Node 22.23.4, bun 1.4.4 and
pnpm 10.34.8 were skipped because they were under two weeks old.

Image environment (`ENV`): `UV_PYTHON_INSTALL_DIR=/opt/uv-python`, `UV_PYTHON_DOWNLOADS=never`, `UV_LINK_MODE=copy`,
`UV_CACHE_DIR=/opt/forge-cache/uv`, `BUN_INSTALL_CACHE_DIR=/opt/forge-cache/bun`, `COREPACK_HOME=/opt/corepack`,
`COREPACK_ENABLE_DOWNLOAD_PROMPT=0`, `COREPACK_ENABLE_AUTO_PIN=0`, `UV_OFFLINE=1`, `LANG/LC_ALL=C.UTF-8`.

**Forge tools pre-warmed offline** (Forge 091c0d3 `src/util.ts`): ast-grep 0.45.3, zizmor 1.30.1, hadolint-bin 2.15.1,
squawk-cli 2.70.0. They are warmed by running the exact invocations of `templates/scripts/lib.sh` (`astgrep`) and
`engine.sh` (`forge_zizmor`, `forge_hadolint`, `forge_squawk`) as user `dev`: `uvx -q --from ast-grep-cli==0.45.3
ast-grep`, `uvx -q zizmor@1.30.1`, `uvx -q --from hadolint-bin==2.15.1 python ...`, `uvx -q --from
squawk-cli==2.70.0 squawk`, `bunx -p @ast-grep/cli@0.45.3 ast-grep`, `bunx -p squawk-cli@2.70.0 squawk`. The caches
are outside `$HOME` (`/opt/forge-cache/{uv,bun}`, owned by dev), so they survive a fresh per-trial `$HOME` and do not
enter the `$HOME` fingerprint. Verified with `docker run --network none --tmpfs /tmp ...`: all six print their
versions offline (bunx ~0.1 s, uvx ~0.03-0.5 s). bunx prints "postinstall script did not run; falling back to runtime
binary resolution" for @ast-grep/cli; it still works.

Rebuild (from `experiment/`; the proxy CA is a BuildKit secret, used for downloads only, not baked in):

```sh
DOCKER_BUILDKIT=1 docker build -f harness/images/base.Dockerfile \
  --secret id=ca,src=/root/.ccr/ca-bundle.crt -t forge-exp/base:v1 harness/images
```

The Python repos need nothing beyond the base, so their yaml uses `image: forge-exp/base:v1` and there are no
`images/<python-repo>.Dockerfile` files. (A rebuild of base gives a new image ID; images built `FROM` an earlier
base must be rebuilt.)

## Containers (`repo_env.start_container`)

`docker run -d --init --user 1000:1000 --workdir /work --cpus 2 --memory 6g --network <none|trialnet|bridge>`,
`sleep infinity`, label `forge-exp.repo_env=1`. `security_extra=True` adds `--security-opt seccomp=unconfined
--security-opt apparmor=unconfined --cap-add SYS_ADMIN` (D7; bubblewrap verified working with it). Env: `HOME`,
the repo yaml `env`, `PATH=<path_prepend>:<image PATH>` (D5), then the caller's `env`.

Caches: every existing `/work/cache/<kind>` is mounted at `/cache/<kind>`:
- **uv (Python repos): per-container overlay.** uv cannot use a read-only cache at all: it creates lock files
  inside the cache (`Could not acquire lock ... Read-only file system`), even with `--offline`. So for kinds in
  `cache_overlay` (default `[uv]` for Python) `start_container` creates a docker `local` volume of type overlay:
  lower = `/work/cache/uv` (the shared cache; overlayfs never writes to a lower layer), upper/work =
  `/work/runs/cache-overlay/<container>/uv/`. The container sees a writable `/cache/uv`; the shared cache is never
  modified. `remove()` deletes the volume and the upper dir.
- other kinds: plain read-only bind mounts.
- fallback: if `/cache/uv` is a plain read-only bind (e.g. a caller mounts it itself), the install commands run
  `uv-cache-overlay` (in the image), which mirrors it into `$UV_CACHE_DIR` as a `cp -as` symlink farm. Works, but
  costs ~15 s per GB of cache, so the overlay volume is the normal path.

`materialize`: `git -C <mirror> archive <commit> | docker exec -i -u dev tar -x -C /work`, then `git init` (branch
`main`), `git add -A -f` (tracked-but-ignored upstream files are kept), one commit "initial", author and committer
`dev <dev@example.com>`, dates `2000-01-01T00:00:00Z`. The commit hash therefore depends only on the tree. It refuses
a tree whose `.gitattributes` uses `export-ignore`/`export-subst` (none of the 8 surveyed repos does).

`run_tests`: `{junit}` is a temp path in the container's `/tmp`, `{targets}` shell-quoted. The JUnit is parsed into
`<file>::<name>`: pytest ids are rebuilt from `classname` + the `file` attribute (`-o junit_family=xunit1`), giving
exact pytest node ids including classes and parameters; vitest ids are `<classname = file>::<name = "describe > test">`.
`error` = `<error>` element (pytest collection/setup/teardown errors); a module that fails to collect is reported as
`<file>` with `error` (vitest: a file that fails to load). `failed` = `<failure>`. Duplicate entries keep the worst
status. Requested ids missing from the result were not run and should be treated as `error`.
`run_tests_detailed` also returns the log, exit code, `junit_found`, `timed_out`.

## Python repos

All three: Python 3.12 (`UV_PYTHON=3.12`; fastmcp's `.python-version`, inside every CI matrix), `uv sync --frozen
--offline`, `path_prepend: /work/.venv/bin`, pytest with `-p no:cacheprovider -o junit_family=xunit1 --junitxml
--continue-on-collection-errors`, no xdist (`-n` never passed), no coverage, `lint_cmd: ruff check --no-cache
--output-format=json` with the repo's own config (ruff from the project venv).

| Repo | Install (mirrors) | Test flags | Notes |
|---|---|---|---|
| pydantic-ai | `make install`: `--all-extras --all-packages --group lint --group docs`, minus heavy local-model extras when the commit defines them (`sentence-transformers`, `outlines-transformers`, `outlines-llamacpp`, `outlines-mlxlm`, `outlines-sglang`, `outlines-vllm-offline`) | none extra (their `make test` adds only `-n auto`, `--durations`) | Excluded extras pull torch + 15 CUDA wheels (~5 GB per venv) or need llama.cpp source builds; their tests skip (`try_import`). Pilot-window commits (May 2026) also define the outlines extras with uv `conflicts`, so `--all-extras` alone fails there; the Makefile of that time excluded `outlines-vllm-offline`. pytest-timeout is not installed in this repo, so no per-test timeout. `COLUMNS=150` as in the Makefile. |
| fastmcp | CI `setup-uv`: `uv sync --locked` (default `dev` group) | `--inline-snapshot=disable --timeout=60 -m "not integration and not conformance"` | CI uses `--timeout=5`; 60 s chosen to avoid spurious timeouts under 2 CPUs and parallel trials (determinism is checked by the 3x validation anyway). integration needs GitHub secrets + network, conformance needs npx + network. |
| logfire | `make install` / CI: `uv sync --frozen` (default `dev` group incl. logfire-api) | none extra | CI also blackholes logfire production hosts; containers are offline anyway. |

No dummy API keys were needed for the smoke tests (pydantic-ai's conftest sets `ALLOW_MODEL_REQUESTS=False` and its
own env fixtures; cassettes are replayed). `CI=true` was deliberately NOT set: the yaml `env` also reaches the agent,
and `CI` can change Claude Code's own behaviour.

### Smoke tests (first main candidate, offline, `python3 harness/repo_env.py smoke ...`)

| Repo | Candidate | At C | At C^ + C's test files |
|---|---|---|---|
| pydantic-ai | 8069baa5df `tests/models/test_bedrock.py` | 138 passed | 1 failed, 137 passed (`test_bedrock_empty_list_tool_result_uses_non_empty_content_block`) |
| fastmcp | cccb529f50 (2 files) | 36 passed | 3 failed, 33 passed |
| logfire | 3cb97d520e `tests/otel_integrations/test_openai.py` | 63 passed | 2 failed, 61 passed |

Typical timings (2 CPUs): materialize ~5 s, offline install 25-50 s, test file 10-30 s.

## Caches (`/work/cache`, filled by `warm_cache.py`)

`python3 harness/warm_cache.py <repo_id> [--limit N] [--pilot 10] [--jobs 2]`: for the first N main + first 10 pilot
candidates, parent and commit, deduplicated by sha256(lockfiles + warm command); runs the yaml `warm` command (the
same `uv sync` online, `UV_CACHE_DIR=/cache/uv` rw, `UV_OFFLINE=false`, proxy CA mounted, bridge network) once per
distinct lockfile, then verifies the offline `install` for the first main candidate's parent (network none). Records
go to `harness/warm/<repo_id>.json` (lock groups: which commits share a lockfile).

| Repo | N main | Commits | Distinct lockfiles | Result |
|---|---|---|---|---|
| pydantic-ai | 60 (+10 pilot) | 106 | 9 | 9/9 ok, offline verify ok |
| fastmcp | 60 (+10 pilot) | 106 | 8 | 8/8 ok, offline verify ok |
| logfire | **40** (+10 pilot) | 83 | 16 | 15/16 ok, offline verify ok; 562dea1a05 fails (see 8) |

logfire bumps its lock very often (27 distinct lockfiles for N=60, ~0.3 GB each); with the shared disk at ~10 GB
free I stopped it and used **N = 40** (the floor allowed). Commits beyond the first 40 main candidates may need a
further warm-up (`--limit 60`) if the dataset needs them and disk allows.

Cache size: `/work/cache/uv` = **8.1 GB** after all three (pydantic-ai ~5 GB of it; shared wheels deduplicated;
~300k files). The verify step also runs, offline in the workspace after install, the four Forge `uvx` invocations,
`uv run python -c ...` (non-frozen `uv run`, which re-checks the lock and rebuilds dynamic-version workspace
metadata) and checks that `uv.lock` is unchanged: all pass for the three repos (`harness/warm/*.json`).
`warm_cache.py` skips lockfiles already recorded as warmed (`warmed_keys`; `--force` to redo) and also warms the
Forge `uvx` tools into `/work/cache/uv`.

## Decisions and problems

1. **`UV_OFFLINE=1` in the image** (both arms). Forge's `astgrep` runs `uvx` online first and falls back to
   `uvx --offline` only after a failure. Offline, whenever the cached tool environment does not match (e.g. the
   project venv's python is first on PATH, as D5 requires) uv retries PyPI for ~6 s first (measured: "Request failed
   after 3 retries in 5.8s"). That would add seconds to many Forge hook calls for a reason that does not exist on a
   developer machine with network. With `UV_OFFLINE=1` uv uses the warm cache directly. It is identical in both arms
   and changes nothing in Forge; the agent's own uv commands that would need the network fail either way (no
   registry access, D4). The coordinator may prefer to remove it; then expect the ~6 s per uncached uvx call.
2. **uv needs a writable cache** -> per-container overlay volumes (above). Contract wording "mounted read-only" is
   kept in substance: the shared host cache is never written by validation/trial containers.
3. **One uv cache for install, agent and Forge (Python repos).** The yaml `env` sets `UV_CACHE_DIR=/cache/uv` (the
   per-container overlay). Without it the agent's `uv run ...` failed offline (it rebuilds the workspace's
   dynamic-version metadata and needs hatchling from the cache), and `uv run pytest` is how these repos document
   running tests. The Forge tools are therefore also warmed into `/work/cache/uv` (by `warm_cache.py`); the image's
   own `/opt/forge-cache/uv` remains the uv cache for the TypeScript repos (Forge's `astgrep` uses uvx there unless a
   `bun.lock` exists). Same in both arms.
4. Heavy pydantic-ai extras excluded (see table). Tests for those integrations skip, so candidates that only touch them
   will have empty F2P and be dropped by validation; this is mechanical and arm-neutral.
5. pydantic-ai's `uv-dynamic-versioning` needs a git repo: `materialize` creates one before `install`; the version
   is derived from the synthetic commit (no tags), so it is deterministic.
6. Candidate `test_files` only include `.py` files. pydantic-ai tests replay VCR cassettes (`tests/**/cassettes/*.yaml`)
   and use `--strict-vcr-cassette-usage`; a commit that adds or changes cassettes needs those files applied with the
   tests (validation/scoring owners: treat non-.py files under `tests/` in the commit as part of the test changes).
7. Disk: the base image is 1.5 GB; each running Python container adds its venv (hundreds of MB to a few GB, copied
   from the cache because `UV_LINK_MODE=copy`) to its writable layer. Removing a container deletes ~50k files and can take tens of seconds
   while docker shows it as "Dead / removal in progress".
8. logfire lockfile of pilot candidates #0-#1 (commits around 562dea1a05, 2026-05-08) pins `mysql-connector-python`
   9.7.0, which has no cp312 wheel; building it needs gcc, which the image deliberately lacks. Those two pilot
   candidates cannot be installed offline; skip them in the pilot (main-window candidates are unaffected).
9. Disk is the binding constraint and is shared with the TypeScript work: free space dipped to 2-4 GB during
   parallel warm-ups. `warm_cache.py` stops scheduling new warm-ups below `--min-free-gb` (default 6).
