# Allow FastMCP errors to choose the log level they are reported at

## Problem

When a tool, resource, resource template or prompt raises a FastMCP error (`ToolError`, `ResourceError`, `PromptError`, or any other `FastMCPError`), the server always logs it at `ERROR` level with a traceback. Many of these errors are expected, such as a missing parameter or a resource that is temporarily unavailable. Authors have no way to report them at a lower severity, so logs fill with spurious errors.

Separately, when a function-based prompt raises a FastMCP error while rendering, the error is caught and re-wrapped in a new `PromptError("Error rendering prompt '<name>': ...")`. This hides the original message. Argument-conversion failures are one example: they produce a `PromptError` such as `Could not convert argument 'numbers' ... list[int] ...`, and the client never sees that text.

## Requirements

### `log_level` on `FastMCPError`

- `fastmcp.exceptions.FastMCPError` accepts an optional keyword-only argument `log_level: int`, defaulting to `logging.ERROR`.
- The value is stored on the instance as the attribute `log_level`.
- Positional arguments behave exactly as before.
- All subclasses inherit this, including `ToolError`, `ResourceError` and `PromptError`. Example: `ToolError("Missing required parameter", log_level=logging.WARNING)`.

### Server logging uses that level

When the server catches a `FastMCPError`, it logs at the error's `log_level` instead of always at `ERROR`. The traceback is still included. The existing log message text stays the same:

| Situation | Log message |
|---|---|
| Calling a tool | `Error calling tool '<name>'` |
| Reading a resource or a resource template | `Error reading resource '<uri>'` |
| Rendering a prompt | `Error rendering prompt '<name>'` |
| Executing a sampling tool via `fastmcp.server.sampling.run.execute_tools` (when the tool raises `ToolError`) | `Error calling sampling tool '<name>'` |

- When a custom level such as `WARNING` is given, these messages must be emitted at that level and **not** additionally at `ERROR`.
- With the default level, behaviour is unchanged: they log at `ERROR`.
- `McpError` and other non-FastMCP exceptions keep their current logging behaviour.

### What clients receive is unchanged

The level only affects logging. Error results and messages delivered to clients stay as they are:

- tool results still have `isError` set and include the message;
- resource reads and prompt gets still raise to the client with the original message.

### Prompt errors pass through unwrapped

- When a function prompt's rendering raises any `FastMCPError`, both of these must propagate the original exception unchanged, without re-wrapping it in an `Error rendering prompt ...` message:
  - `Prompt.render`
  - the server's prompt get
- For example, a conversion failure surfaces as a `PromptError` containing `Could not convert argument '<arg>'` and the target type. Clients see the same message.
- Other, non-FastMCP exceptions are still wrapped as before.
