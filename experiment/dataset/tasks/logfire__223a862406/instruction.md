You are working in the git repository in the current directory. Make the change described below. Work autonomously: nobody will answer questions.

# Remote variables provider: ETag conditional GETs, silent 304s, and a post-SSE follow-up refresh

## Problem

`LogfireRemoteVariableProvider` (in `logfire.variables.remote`) downloads the full `/v1/variables/` body on every poll, even when nothing has changed.

Also, when an SSE notification triggers a forced refresh, the first fetch can be answered by a server-side cache with stale data. The provider then stays stale until the next regular poll, which may be a full polling interval away.

## Requirements

### Conditional GET / ETag

- The provider tracks the last validator in a new attribute `_etag: str | None`. It starts as `None`.
- When `_etag` is not `None`, `refresh()` sends `If-None-Match: <etag>` on the `GET /v1/variables/` request.
- After a successful 200 response, `_etag` is set to the response's `ETag` header value.
  - If the response has no `ETag` header, `_etag` is cleared to `None`, so a stale validator is never reused.
- `_etag` is only updated once the response body has validated successfully as a `VariablesConfig`.
  - A malformed 200 that fails validation must leave the previous `_etag` unchanged, along with the previous config.

### 304 Not Modified

A 304 response counts as a successful round-trip. It must:

- leave `_config` untouched (the same object);
- update `_last_fetched_at` to the current UTC time;
- reset `_consecutive_refresh_failures` to `0`;
- mark that a fetch has been attempted;
- not call the error logger and not emit any warning.

### Follow-up refresh after SSE-triggered fetch

- A new attribute `_followup_refresh_at: float | None` holds a deadline as a `time.monotonic()` value. It starts as `None`.
- In the worker loop (`_worker`), after an iteration whose refresh was forced by an SSE wakeup (`_force_refresh_event`), set `_followup_refresh_at` to about `time.monotonic() + 2.0`.
- While a deadline is pending, the jittered wait on `_worker_awaken` is clamped so the worker wakes no later than the deadline. The clamp is never negative.
- After the wait returns:
  1. If `_shutdown` is set, the loop breaks immediately, before any follow-up handling.
  2. Otherwise, if the deadline has passed, `_followup_refresh_at` is reset to `None`. No extra fetch is made at this point; the next iteration's normal top-of-loop refresh serves as the follow-up.
- Example: with an already-elapsed deadline and two loop iterations before shutdown, `refresh` is called exactly twice. If shutdown occurs during the first wait, `refresh` is called exactly once.
- The follow-up request still sends `If-None-Match` when an ETag is known.

### Fork safety

When the worker thread is restarted in a forked child process, `_followup_refresh_at` is cleared to `None`. The existing reconnect behaviour stays as it is.
