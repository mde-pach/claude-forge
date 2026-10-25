You are working in the git repository in the current directory. Make the change described below. Work autonomously: nobody will answer questions.

# Streamable-HTTP: let the SDK session manager own the server lifespan, and add session idle timeouts

## Problem

When FastMCP serves over streamable HTTP, the FastMCP server lifespan is entered separately from the MCP SDK's `StreamableHTTPSessionManager`. The SDK-level lifespan hook only reads back a result that someone else must already have produced. If nothing did, it raises a `RuntimeError` saying the server's context manager was not entered.

Because the session manager does not drive the lifecycle, two SDK features do not work on FastMCP's stateful HTTP path:

1. **Session-owner enforcement.** A streamable-HTTP session should be bound to the credential that created it. Today, a leaked `Mcp-Session-Id` can be used with any valid bearer token.
2. **Idle session expiry.** There is no way to terminate sessions that have gone idle.

## Required behaviour

### Lifespan

- Entering the low-level MCP server's lifespan (the SDK's `server.lifespan(server)`, as used by `StreamableHTTPSessionManager.run()` and `Server.run()`) must itself run the FastMCP server lifespan. It must not require that lifespan to have been entered beforehand, and must not raise in that case.
- The lifespan must never run twice. If an outer caller such as `run_http_async` or `run_stdio_async` has already entered it, a nested entry reuses the existing result and does not repeat setup or teardown.
- The streamable HTTP app's own lifespan must enter the FastMCP lifespan exactly once.

### Session ownership

With auth configured on a stateful streamable-HTTP app:

- A session initialized with token A can be reused with token A, which returns 200.
- Reusing that session with a different valid token B must return **404**, the same response as if the session did not exist.

### Session idle timeout

Add a `session_idle_timeout: float | None = None` keyword parameter, measured in seconds, to:

- `fastmcp.server.http.create_streamable_http_app(...)`, which passes it to the SDK `StreamableHTTPSessionManager`.
- `FastMCP.http_app(...)`. When this is `None`, it falls back to the new setting `fastmcp.settings.http_session_idle_timeout`. The value is only used for the streamable-HTTP transport.

Add a `Settings` field `http_session_idle_timeout: float | None`:

- Default `None`, meaning sessions never expire from inactivity.
- When set, it must be strictly greater than 0. Otherwise validation fails.
- It can be set from the environment like the other `http_*` settings.

Semantics:

- A session idle for longer than the timeout is terminated and removed from the session manager's active sessions.
- Every request on the session pushes the deadline forward.
- After expiry, a request using the old `mcp-session-id` returns **404**.
- Idle timeout is not supported in stateless HTTP mode.
