Turborepo 2: tasks are under `tasks` in `turbo.json` (not `pipeline`), and env mode stays strict.
Every environment variable the code reads is declared in `turbo.json` `env` (per task) or `globalEnv`, so it is part of the cache key.
