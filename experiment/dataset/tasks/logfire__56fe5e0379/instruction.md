You are working in the git repository in the current directory. Make the change described below. Work autonomously: nobody will answer questions.

# Harden SSE reconnect and polling behaviour in `LogfireRemoteVariableProvider`

`LogfireRemoteVariableProvider` (in `logfire.variables.remote`) keeps variables up to date with a background polling worker and an SSE listener. It has three reliability problems.

## 1. Events missed while the SSE stream was down are never fetched

When the SSE connection drops and later reconnects, any variable updates published during the outage are not picked up until the next scheduled poll.

**Required:**
- The provider tracks whether it has ever had a successful SSE connection, in a boolean attribute `_sse_had_connected`. It is `False` on construction, and `_force_refresh_event` is not set on construction.
- On every successful (HTTP 200) SSE connection after the first, the listener requests a forced refresh. It does this by setting `_force_refresh_event` and waking the worker through `_worker_awaken`.
- The first-ever connection does not trigger this refresh.
- `_at_fork_reinit()` must not reset `_sse_had_connected`. A child process's first reconnect after a fork counts as a reconnect and must trigger the forced refresh.

## 2. Reconnect backoff handling is wrong for keepalive-only streams

The reconnect backoff starts at 1s, is passed to `_wait_for_reconnect(delay)` before each retry, and doubles after each wait. Currently it is reset only when a `data:` line arrives. A healthy stream that sends only keepalive comments therefore keeps backing off.

**Required:**
- The backoff resets to 1.0 when the stream delivers any SSE-framed line. That is a stripped line starting with `:`, `data:`, `event:`, `id:` or `retry:`.
- A 200 response whose stream yields no lines must **not** reset the backoff.
- A 200 response whose lines are not SSE-framed (e.g. an HTML error page from a proxy) must **not** reset the backoff either.

For example, the following sequence must produce reconnect waits of `1.0, 2.0, 4.0, 1.0`:
1. a 401 response
2. an empty 200 stream
3. a 200 stream with an HTML body
4. a 200 stream containing only `: keepalive`

## 3. Fleets poll in lockstep, and the interval gate can skip polls

**Required:**
- Each worker iteration waits on `_worker_awaken.wait(timeout)` with a jittered timeout. The timeout is uniformly random in `[0.9, 1.1] × polling_interval` (in seconds).
- Every worker iteration calls `refresh(force=True)`, including ordinary scheduled iterations with no pending force event. Without this, a jittered wait shorter than the base interval would be skipped by `refresh`'s interval gate.
- Clearing `_force_refresh_event` when it is set still happens as before.
