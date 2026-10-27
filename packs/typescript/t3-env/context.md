Environment variables are read through the validated `env` object of the env module (`createEnv`), never `process.env` elsewhere; tool config files (`*.config.*`) are exempt.
