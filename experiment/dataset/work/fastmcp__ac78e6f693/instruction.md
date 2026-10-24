# Single SERVER span per request, and spec-correct error codes

## Problem

**Tracing.** When a request goes through the real dispatch path (for example via `fastmcp.Client`), the server can produce two SERVER spans for the same request:

- one from the MCP SDK's built-in OpenTelemetry middleware;
- one from FastMCP's own telemetry.

The two spans use different attribute conventions.

There is also a gap in the other direction. FastMCP's own spans are only created deep in the tool, resource and prompt path. So these requests get no FastMCP SERVER span at all:

- methods that never reach that path, such as `logging/setLevel`, `ping`, `initialize` and `tasks/*`;
- requests rejected before the handler, for example by a raising FastMCP middleware or a not-found component.

**Error codes.**

- Reading a missing resource returns JSON-RPC error code `-32002`. Per the MCP spec (SEP-2164) it should be `INVALID_PARAMS` (`-32602`).
- Mapping from FastMCP exceptions to wire error codes is done by hand at each call site, so the codes are inconsistent.

## Requirements

### Tracing

- Every inbound request handled through the server dispatcher produces exactly one SERVER span. Notifications produce no SERVER span. The SDK's own OpenTelemetry middleware must no longer emit a duplicate span.
- For `tools/call`, `resources/read` and `prompts/get`, that single span:
  - keeps the existing rich name, e.g. `tools/call greet`;
  - keeps the existing attributes: `mcp.method.name`, `fastmcp.server.name`, `fastmcp.component.type`, `fastmcp.component.key` (e.g. `tool:greet@`), `gen_ai.tool.name`, and so on.
- For other methods, the span is named after the method (e.g. `logging/setLevel`). It carries `mcp.method.name`, `fastmcp.server.name`, and the auth and session attributes.
- If a request is rejected before reaching the handler, it still gets its one SERVER span with `mcp.method.name` set.
  - If the exception propagates out (e.g. a `RuntimeError` raised in middleware), the span has ERROR status, `error.type` set to the exception's qualified name (`"RuntimeError"`), and a recorded exception event.
  - A `ToolError` returned to the client as an error result still yields exactly one span.
- The server span must still join the client's distributed trace through W3C context in `_meta`: same trace ID, with a parent.
- In-process calls that bypass the dispatcher (e.g. `mcp.call_tool(...)`) continue to create their own SERVER span as before.
- `fastmcp.server.telemetry` additionally exports:
  - `seam_span(method, server_name)`, a context manager;
  - `record_span_exception(span, exc)`;
  - `SEAM_SPAN_MARKER = "fastmcp.span.seam"`.

### Errors

- Add `fastmcp.exceptions.to_mcp_error(exc: Exception, *, default_code: int = INTERNAL_ERROR) -> MCPError`. Here `INTERNAL_ERROR` is the constant from `mcp_types`. It maps exceptions as follows:
  - `NotFoundError`, `DisabledError` and `ValidationError` map to `INVALID_PARAMS`.
  - Any other exception (e.g. `ToolError`, `ResourceError`, `PromptError`) maps to `default_code`.
  - An `MCPError` is returned unchanged, as the same object.
  - The resulting `.error.message` equals `str(exc)`.
- The fallback `MCPError` class, used when MCP dependencies are not installed, must accept `(code, message, data=None)` and expose `.error` as an `ErrorData`.
- Wire errors:
  - Reading a missing resource fails with code `INVALID_PARAMS` and a message containing `Resource not found`.
  - Getting an unknown prompt fails with code `INVALID_PARAMS` and a message containing `Unknown prompt`.
  - Pagination cursor errors remain `INVALID_PARAMS`.
