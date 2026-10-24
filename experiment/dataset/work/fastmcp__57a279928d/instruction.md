# Protect streamable HTTP transport from DNS rebinding

## Problem

Apps built with `fastmcp.server.http.create_streamable_http_app` (and `FastMCP.http_app()` / `FastMCP.run_http_async()` using the `http`/`streamable-http` transport) accept requests regardless of their `Host` or `Origin` headers. A malicious web page can use DNS rebinding to reach a locally running MCP server and initialize a session. Requests with an untrusted Host or Origin must be rejected before they reach the MCP endpoint, so no session is created and no `mcp-session-id` header is returned.

## Required behaviour

Host/Origin protection is **on by default** for streamable HTTP apps. It applies to every HTTP request (non-HTTP ASGI scopes pass through).

**Host validation.** The `Host` header (port ignored; IPv6 in brackets handled; case-insensitive) must match one of:
- the defaults `127.0.0.1`, `localhost`, `::1`;
- any entry in `allowed_hosts` (shell-style wildcards supported; `*` allows any host);
- the host the server is bound to, unless it is an unspecified address such as `0.0.0.0`.

Otherwise respond with status **421** and body `Misdirected Request`.

**Origin validation.** If an `Origin` header is present, it is accepted when any of these is true:
- it matches an entry in `allowed_origins` (wildcards supported; `*` allows any);
- both the origin's host and the request's Host are loopback (e.g. origin `http://localhost:3000` against Host `127.0.0.1`);
- it equals the request's own origin (request scheme + Host), comparing scheme, host and port after normalisation. Default ports count as 80/443, so `https://mcp.example.com` matches `https://mcp.example.com:443`. A different scheme or port does not match.

Otherwise respond with status **403** and body `Forbidden Origin`. A loopback origin is rejected for a non-loopback Host unless it is listed in `allowed_origins`. Requests with no `Origin` header skip this check.

## Configuration

`create_streamable_http_app` gains these keyword parameters:
- `host_origin_protection: bool = True`
- `allowed_hosts: Sequence[str] | None = None`
- `allowed_origins: Sequence[str] | None = None`

When `host_origin_protection=False`, no Host/Origin validation is done. This guard does not configure CORS. CORS must be configured separately when browser JavaScript has to read cross-origin responses.

`FastMCP.http_app(...)` and `FastMCP.run_http_async(...)` accept the same three parameters, each defaulting to `None`. For streamable HTTP, a `None` value falls back to these new settings on `fastmcp.settings`:
- `http_host_origin_protection: bool = True`
- `http_allowed_hosts: list[str] | None = None`
- `http_allowed_origins: list[str] | None = None`

These settings can be set through the usual `FASTMCP_` environment variables. The SSE transport is unchanged.
