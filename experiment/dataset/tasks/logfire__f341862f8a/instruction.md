You are working in the git repository in the current directory. Make the change described below. Work autonomously: nobody will answer questions.

# Variables refresh: transient failures should warn, not error; provider traffic should not be instrumented

## Problem

`LogfireRemoteVariableProvider` (in `logfire.variables.remote`) polls the Logfire API for variables. Today, every failed refresh is logged at error level with the exception attached. This includes a single 503, a timeout or a connection reset during a rolling deploy. The provider keeps serving the previously fetched config and retries on the next poll, so these failures have little impact. Logging them as errors creates noise and inflates error rates.

Separately, users who instrument their HTTP client (for example with `logfire.instrument_requests()`) get a span for every provider poll and API write. They also get spans for SSE reconnects. This traffic is Logfire's own internal work and should not appear in user telemetry.

## Requirements

### Log level for refresh failures

- Add a module-level constant `_CONSECUTIVE_FAILURES_BEFORE_ERROR = 5` in `logfire.variables.remote`.
- The provider tracks consecutive refresh failures in an integer attribute `_consecutive_refresh_failures`:
  - It starts at `0`.
  - It increases by 1 on each failed fetch.
  - It resets to `0` after any successful fetch.
- A failed refresh is logged as a **warning** only when all three of these are true:
  - a config was previously fetched successfully;
  - the failure count, including this failure, is below `_CONSECUTIVE_FAILURES_BEFORE_ERROR`;
  - the error is transient.
- In all other cases the failure is logged at error level, as it is today, with the message `'Error retrieving variables'` passed as the `message` keyword.
- An error counts as transient if it is:
  - an HTTP response with status ≥ 500, 408 or 429; or
  - any other `requests.RequestException`, such as connection errors and timeouts.

  Other HTTP statuses are not transient and log an error immediately, even when a cached config exists. Examples are 401 and 403.
- When a bound logfire instance exists, the warning is emitted via its `warn` method:
  - The message template is `'{message}: {error_type}: {error}'`.
  - It takes these keyword arguments:
    - `message='Error retrieving variables'`
    - `error_type=` the exception's class name, for example `'UnexpectedResponse'`
    - `error=` `str(exc)`
  - The exception is **not** attached, so no `_exc_info` is passed.
- When no logfire instance is bound, emit a `RuntimeWarning` with the text `'Error retrieving variables: <ExcType>: <exc>'`.

### Instrumentation suppression

- None of the provider's own HTTP requests should produce spans when the user has instrumented `requests`. This covers:
  - variables GET, POST, PUT and DELETE;
  - variable-types GET and POST;
  - the SSE listener connection.
- Suppression must apply only to the provider's own requests. A user's own `requests` calls made after a provider refresh must still be instrumented as usual.
- Refresh failures must still be logged, so suppression must not swallow the warning or error records.
