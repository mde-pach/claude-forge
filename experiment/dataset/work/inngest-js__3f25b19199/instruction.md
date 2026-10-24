## Sub-second durations in durable waits are silently dropped

### Problem

Durable waits (`step.sleep`, `step.waitForEvent` timeouts, `step.waitForSignal` timeouts, `step.invoke` timeouts, and function `cancelOn` timeouts) accept durations as a number of milliseconds or as an `ms`-compatible string. Before they are sent to the server, these durations are converted to whole seconds by rounding down. A duration under one second, such as `500` or `"500ms"`, therefore becomes the empty string `""`. The server treats that as a zero-length wait.

The result is that `step.sleep("id", 500)` resolves at once, and `step.waitForEvent(..., { timeout: 500 })` times out at once. No error or warning is shown.

### Required behaviour

The time-string helper `timeStr` lives in `packages/inngest/src/helpers/strings.ts`. It converts a `number`, an `ms` string, a `Date`, or a Temporal value into the server's time-string format, such as `"1d"` or `"2h30m10s"`. It must behave as follows:

- **Durations under 1s:** any duration greater than 0 ms and less than 1000 ms rounds up to `"1s"`. This includes fractional values. For example, `1`, `500`, `"500ms"`, `999`, `999.6` and `0.4` all produce `"1s"`.
- **Durations of 1s or more:** these still round down to whole seconds, as before.
  - `1500` → `"1s"`
  - `65500` → `"1m5s"`
  - `1000` → `"1s"`
  - `60000` → `"1m"`
  - A `Temporal.Duration` of 1 hour plus any sub-second part → `"1h"`
- **Zero and negative durations:** these still return `""`.
- **Positive durations:** a duration of at least 1 ms never produces `""`.
- **`Date` and absolute Temporal inputs:** these are unchanged and still produce ISO strings.

### Logging

`timeStr` gains an optional second parameter, `logger?: Logger`, where `Logger` is the SDK's logger type with `info`, `warn`, `error` and `debug`. Its signature becomes `timeStr(input, logger?)`.

When a logger is given and the round-up to 1s happens, `timeStr` calls `logger.warn` with two arguments:

1. the object `{ requestedMs: <original milliseconds> }`
2. a message containing the phrase `round up to 1s`, for example: `"Durable waits have a minimum resolution of 1s; durations under 1s round up to 1s"`

This warning is logged **at most once per process**, no matter how many times the round-up happens later. Inputs that are not rounded up never log a warning.

Every SDK call site that serializes a durable-wait duration should pass the client's internal logger, so users see this warning. Those call sites are sleep, waitForEvent, waitForSignal, invoke timeouts and cancellation timeouts.

### Observable outcomes

- `await step.sleep("id", 500)` returns an op whose `name` is `"1s"`, and in a real run it sleeps about 1 second.
- `await step.waitForEvent("id", { event: "event", timeout: 500 })` returns an op whose `opts.timeout` is `"1s"`. If no event arrives, it resolves to `null` after about 1 second.

### Documentation

Update the JSDoc for the affected `timeout` and sleep duration options to say that durations under 1 second round up to `1s`, the minimum resolution of a durable wait.
