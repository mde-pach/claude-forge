# Support aborting a deferred run via `defer(...)` handle

## Problem

The experimental `defer(id, { function, data, experiment? })` helper available on the function context triggers a deferred function (created with `createDefer` from `inngest/experimental`) by emitting a `DeferAdd` op. Once called, there is currently no way to cancel it. `defer` returns `void`.

## Requirements

### Return a handle

`defer(...)` must return a `DeferHandle`. This type is exported from the package's types alongside `DeferFn`:

```ts
type DeferHandle = { abort: () => void };
```

The `DeferFn` type's return type changes from `void` to `DeferHandle`.

- `defer` must always return a handle, even when the defer itself is skipped. Skips happen when:
  - the function wasn't created via `createDefer`;
  - schema validation fails or is async;
  - any unexpected error occurs.

  In these cases `abort()` is a no-op.
- `abort()` is synchronous and fire-and-forget. It must never throw into user code. Unexpected errors are logged, not propagated.

### Abort semantics

- Calling `abort()` emits a sync op with opcode `StepOpCode.DeferAbort`.
  - Its `opts` include `target_hashed_id`, which is the hashed step ID of the original defer (i.e. `_internals.hashId(id)`).
  - Its `opts` also include `fn_slug` (the deferred function's slug) and `id` (the original, unhashed defer ID).
  - The abort op's own ID is derived from the target, for example by hashing `"<hashedId>:abort"`.
- **Idempotent.** Calling `abort()` more than once for the same defer within an execution must produce only one `DeferAbort` op.
- **Co-batched add + abort.** Suppose a `DeferAdd` for a defer hasn't been shipped yet when its `DeferAbort` is buffered, for example when `defer(...)` and `abort()` are called back-to-back, or both happen inside the same `step.run`. In that case the pending `DeferAdd` must be dropped. The outgoing ops contain only the `DeferAbort` for that defer, never both.
- **Already-shipped add.** If the `DeferAdd` already shipped in an earlier request, aborting later must still prevent the deferred run. This applies both after another step and from inside a `step.run` closure.
- Aborting one defer must not affect other, unaborted defers in the same run.

### Executor-provided defer state

The request payload's `defers` map (keyed by hashed defer ID) now has entries of shape `{ abortable?: boolean }`. The execution option `priorDefers` accepts `Record<string, { abortable?: boolean }>`.

If the entry for a defer has `abortable: false`, then `abort()` for that defer is a no-op and no `DeferAbort` op is emitted.

### Checkpointing

The behaviour above must hold both with checkpointing enabled and with it disabled. This includes `defer` called inside a `step.run`, with the step later replayed after re-entry.
