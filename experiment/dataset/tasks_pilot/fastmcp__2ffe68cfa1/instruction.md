You are working in the git repository in the current directory. Make the change described below. Work autonomously: nobody will answer questions.

## `from_function` ignores metadata attached by the standalone `@tool` decorator

### Problem

You can use the standalone decorator `fastmcp.tools.tool` on a plain function without a server. The decorator stores its configuration on the function in a `ToolMeta` object, which it exposes as `fn.__fastmcp__`. That configuration includes name, version, title, description, tags, meta and so on.

When that decorated function is turned into a tool directly with `Tool.from_function(fn)` or `FunctionTool.from_function(fn)`, the decorator's configuration is discarded. The resulting tool falls back to defaults derived from the function itself:

- the name comes from the function's `__name__`;
- the description comes from the docstring;
- no version, title, tags or meta are set.

Example:

```python
from fastmcp.tools import tool
from fastmcp.tools.base import Tool

@tool(
    name="custom-greet",
    version="v1",
    title="Greeting Tool",
    description="Greets people",
    tags={"greeting", "demo"},
    meta={"custom": "value"},
)
def greet(name: str) -> str:
    """Fallback description."""
    return f"Hello, {name}!"

t = Tool.from_function(greet)
t.name         # currently "greet"; should be "custom-greet"
t.description  # currently "Fallback description."; should be "Greets people"
```

### Required behaviour

The rule applies to both `Tool.from_function` (in `fastmcp.tools.base`) and `FunctionTool.from_function` (in `fastmcp.tools.function_tool`). It covers the case where the caller passes no `metadata` argument and no individual metadata keyword arguments (such as `name`, `description`, `tags` or `version`).

In that case, if the function carries a `ToolMeta` attached by the `@tool` decorator, the created tool must use that metadata. The resulting tool's `name`, `version`, `title`, `description`, `tags` and `meta` attributes must equal the values given to the decorator. The other fields stored in `ToolMeta` must also be honoured.

The following behaviour must stay unchanged:

- If the caller passes `metadata=` explicitly, or passes any individual metadata keyword arguments, those take effect as they do today. Decorator metadata is not merged in.
- Passing both `metadata` and individual parameters must still raise the same error as before.
- Functions without decorator metadata, or whose `__fastmcp__` attribute is not a `ToolMeta`, must behave exactly as before.
