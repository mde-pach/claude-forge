## Durable-execution MCP `get_tools` caching breaks replay determinism

### Problem

The Temporal and DBOS MCP toolset wrappers (`TemporalMCPToolset`, `DBOSMCPToolset`) cache tool definitions on the wrapper instance. That instance is shared across every run in a worker process. When the wrapped toolset has `cache_tools=True`, the first run in a worker schedules the `get_tools` activity/step. Every later run in the same process finds the cache already warm and schedules no `get_tools` activity/step at all.

As a result, the recorded sequence of activities/steps depends on what earlier runs did in that worker, not only on the workflow's own history. On replay or recovery in a fresh worker, the sequence differs:

- Temporal raises a non-determinism error (TMPRL1100).
- DBOS raises `DBOSUnexpectedStepError`.

### Required behaviour

- The tool-definition cache must be scoped to a single agent run, not to the wrapper instance.
  - It is created fresh for each run.
  - It is rebuilt identically on replay or recovery.
- Within one run, with `cache_tools=True`, the wrapped toolset's `get_tools` activity/step runs at most once. Later model requests in that run reuse the cached definitions without scheduling another activity/step.
- Two runs executed back-to-back in the same worker must each record their own single `get_tools` activity/step at the start. For example, a DBOS run produces exactly one `<agent>__mcp_server__<id>.get_tools` step, and it is the first step.
- With `cache_tools=False`, every `get_tools` call schedules the activity/step, as it does today.
- Temporal workflow histories must replay successfully, including with `temporalio.worker.Replayer` and `UnsandboxedWorkflowRunner`.

### Interface

- `RunContext` (`pydantic_ai.tools.RunContext`) gains a private field:
  - Name and type: `_mcp_tool_defs_cache: dict[str, dict[str, ToolDefinition]]`
  - Default: an empty dict, created per instance
  - Excluded from `repr`
  - Keyed by the toolset `id`, using `''` when `id` is `None`
- During an agent run, every `RunContext` built for that run shares the same dict object by reference.
- `RunContext(deps=..., model=..., usage=...)` built directly must have its own empty cache.
- The Temporal and DBOS MCP wrappers read and write this cache in `get_tools`.
  - If an entry exists for the toolset's id and caching is enabled, `get_tools` returns tools built from it without invoking the activity/step.
  - This must also work when the cache was pre-populated manually on a standalone `RunContext`.
- `TemporalMCPToolsetBase` and `DBOSMCPToolsetBase` each gain an abstract read-only property `_cache_tools -> bool`.
  - It reports whether the wrapped toolset has caching enabled.
  - Concrete subclasses must implement it. `TemporalMCPToolset` and `DBOSMCPToolset` return the wrapped `MCPToolset.cache_tools`.
- The instance attribute `_cached_tool_defs` on the wrappers is removed.
