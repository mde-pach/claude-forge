You are working in the git repository in the current directory. Make the change described below. Work autonomously: nobody will answer questions.

## `Shared()` dependencies fail on servers with task-enabled components

### Problem

`Shared(...)` dependencies (from `fastmcp.dependencies`) are app-scoped. They should be resolved once for the lifetime of the server and reused across requests.

This works on an ordinary server. It breaks when both of these are true:

- `pydocket` is installed, so `fastmcp.server.dependencies.is_docket_available()` returns `True`.
- The server has at least one task-enabled component, for example a tool declared with `@mcp.tool(task=True)`.

In that setup the server starts its lifespan with the Docket/Worker background-task machinery. On that path the app-scoped shared-dependency context is never set up or captured. Ordinary, non-task calls to tools that use `Shared(...)` then fail to resolve the dependency.

### Expected behaviour

A server must support `Shared()` dependencies the same way whether or not it has task-enabled components. Example:

```python
mcp = FastMCP("task-capable-server")

@mcp.tool(task=True)
async def background_tool(x: int) -> int:
    return x

@mcp.tool()
async def normal_tool(config: dict[str, str] = Shared(get_config)) -> str:
    return config["key"]
```

When a `Client(mcp)` calls `normal_tool` more than once within a single session:

- Each call succeeds.
- Each call returns the value produced by the shared factory. For a factory returning `{"key": "value"}`, the text content is `"value"`.
- The factory (`get_config`) is invoked exactly once across all of those requests, because the value is app-scoped and reused.

### Requirements

- When the lifespan takes the Docket-enabled path, it must establish the app-scoped shared-dependency context.
- That context must be captured so it is re-applied to every incoming request, just as it is on the non-Docket path.
- Behaviour must not change for servers without task-enabled components, or when `pydocket` is not installed.
- No public API changes are needed. `Shared`, `FastMCP`, `Client`, and `is_docket_available` keep their current signatures.
