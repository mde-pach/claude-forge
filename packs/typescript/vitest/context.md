Vitest 4. Several test setups are `test.projects` in `vitest.config.*`; a `vitest.workspace.*` file is ignored. Parallelism is set with `maxWorkers`.
Tests assert behaviour: never weaken, remove or skip an assertion to make a test pass.
When the task changes behaviour an existing test asserts, update that test in place with Edit or Write (keep its name; Forge asks for approval) and say so in your summary. Otherwise never edit an existing test to make the code pass.
