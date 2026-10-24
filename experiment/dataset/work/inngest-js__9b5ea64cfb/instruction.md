# Streaming responses crash the process when the consumer disconnects

## Problem

When a handler is served with `serve({ streaming: true })`, the SDK sends a streaming response for POST requests. A periodic heartbeat writes padding to the stream to keep it alive until the final result is written.

If the consumer disconnects before the response finishes, the runtime cancels the response stream and closes it. The heartbeat timer keeps running and tries to write to the closed stream. That write throws an uncaught `TypeError` (`Invalid state: Controller is already closed`), and the exception crashes the Node process. Finalizing the stream after a disconnect has a similar problem: it leads to an unhandled promise rejection.

## Requirements

The stream helper is `createStream`, exported from `packages/inngest/src/helpers/stream.ts`. It returns `Promise<{ stream: ReadableStream; finalize: (data: unknown) => void }>`. Its existing options (`interval`, `value`) stay the same. It must meet these requirements:

- **Heartbeat stops on cancel.** After the consumer cancels the returned `stream` (for example with `stream.getReader().cancel()`), the heartbeat must stop. Advancing timers past any number of heartbeat intervals must not throw.
- **Heartbeat write failures are contained.** Some runtimes do not reliably signal cancellation. If a heartbeat write fails anyway:
  - the error must be caught, not thrown from the timer;
  - the heartbeat must stop;
  - no further writes may be attempted.
- **Finalize is safe after cancel.** Calling `finalize(data)` after the stream has been cancelled or closed must not throw. It must not cause an unhandled promise rejection, including once any promise passed as `data` resolves. If the stream is already closed, `finalize` should do nothing. Any other failure while writing the final value must also be caught.
- **New optional `logger` option.** `createStream` accepts `opts.logger`, using the SDK's `Logger` type. When a write failure is caught that `cancel` did not already account for, it is reported at `debug` level through this logger:
  - a failed heartbeat write is logged as `"Failed to write heartbeat to stream"`;
  - a failed final write is logged as `"Failed to write final value to stream"`.

  Both messages include the error. If no logger is given, these failures are silently ignored.
- **Normal behaviour is unchanged.** Without a disconnect:
  - heartbeats are emitted at the configured interval;
  - `finalize` writes the stringified (awaited) data;
  - the stream closes after that write.
- **Serve handler passes its logger.** The serve handler's streaming POST path must pass the client's internal logger to `createStream`.
