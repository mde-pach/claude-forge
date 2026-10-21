Vitest 4. Several test setups are `test.projects` in `vitest.config.*`; a `vitest.workspace.*` file is ignored. Parallelism is set with `maxWorkers`.
Tests assert behaviour: never weaken, remove or skip an assertion to make a test pass.
If an existing test looks wrong, stop and ask; never edit it to make the code pass.
