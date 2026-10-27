Effect: inside `Effect.gen`, effects are composed with `yield*`; `Effect.runPromise` / `runSync` / `runFork` run only at the edge of the program, never inside a generator.
Failures are typed: `yield* Effect.fail(new MyError())` (a `Data.TaggedError`), not `throw`, inside `Effect.gen`.
