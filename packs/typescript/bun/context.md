Tests that import from `bun:test` run with `bun test`. In them, new `.skip`, `.only`, `.todo`, `.failing`, `.if`/`.skipIf`/`.todoIf`, fewer `expect` calls and new mocks (`mock`, `mock.module`, `spyOn`) are rejected.
If an existing test looks wrong, stop and ask; never edit it to make the code pass.
