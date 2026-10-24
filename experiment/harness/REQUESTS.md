
## From the TS repo environments (better-auth, zod, inngest-js), 2026-10-24
None of these block the TS repos; all are worked around in the yaml files (see IMAGES_TS.md).
1. **warm_cache.py low-disk cancel:** an earlier version crashed with `CancelledError` when it cancelled after
   low disk (inngest-js run). The current code has `if f.cancelled(): continue`, which looks fixed. Note that the
   free-disk check is global: other agents' transient warm containers triggered it at 3.6 GB free.
2. **Validation and scoring order for better-auth:** `test_cmd` rebuilds the targeted workspace packages (turbo,
   cached) before vitest, and exits 3 with no JUnit and the marker `[forge-exp] workspace build failed; tests not run`
   when the build fails. classify.py should treat that as a *fail*: the agent's code does not build. It is not
   infra. The first test run after the hidden test files are written takes about 15 s longer (a rebuild of that
   package).
3. **Concurrency:** a better-auth workspace is about 3 GB in the container layer and its `install` takes about
   6 to 8 min at the repo_env limit of 2 CPUs (mostly `pnpm build`). Schedule at most 2 better-auth containers
   at once.
