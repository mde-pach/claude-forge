## Slow async callback warnings should be parented to the span active in the blocking task

`logfire.log_slow_async_callbacks(slow_duration=...)` emits a warning log whenever an asyncio callback or task step blocks the event loop for longer than the threshold. The message template is `'Async {name} blocked for {duration:.3f} seconds'`, emitted at warn level.

### Problem

These warnings are always emitted as orphan root spans (`parent: None`), each in its own new trace. This happens even when the blocking code ran inside an active span.

For example:

```python
async def bar():
    with logfire.span('in bar'):
        await foo()
        blocking_call()   # blocks the loop for 2s
```

Here the warning for `task bar 1 (bar)` should appear as a child of the `in bar` span, in the same trace. Today it starts a separate, unrelated trace. That makes it hard to see which operation caused the event loop to block.

The cause is that the warning is emitted from the event loop's own context after the callback finishes. It is not emitted in the context of the callback or task that was running.

### Required behaviour

- **Parenting:** each slow-callback warning must be emitted within the context of the callback or task that blocked. The context is the one the asyncio handle runs in. As a result:
  - If a span was active in that task at the point it yielded or returned, the warning's parent is that span and it shares its trace ID.
  - If no span was active, the warning remains a root span with `parent: None` in its own trace, as before.
- **Unchanged:**
  - message template and formatted message, for example `Async task bar 1 (bar) blocked for 2.000 seconds`;
  - level;
  - attributes: `duration`, `name`, `stack`, `code.*`, `logfire.json_schema`;
  - instrumentation scope name `logfire.asyncio`.
- **Restoration:** the patching must still be undone when the returned context manager exits.
- **Exceptions:** an exception raised while emitting the warning must still be swallowed, as before, and must not break the event loop.
