You are working in the git repository in the current directory. Make the change described below. Work autonomously: nobody will answer questions.

## Discovered tool-search tools get rejected by OpenAI models without native `tool_search`

### Problem

Some tools belong to the tool-search corpus. This includes tools registered with `defer_loading=True` while the `ToolSearch()` capability is active, and tools revealed by loading a deferred `Capability(..., defer_loading=True)`.

When the model has no native tool search, search falls back to the local `search_tools` function tool. One example is `OpenAIResponsesModel('gpt-5.2', ...)`, which predates OpenAI's native `tool_search`.

Once such a tool has been discovered, it is sent to the provider as a callable tool. The trouble is that it still carries its native tool-search association. The OpenAI Responses adapter therefore stamps `defer_loading` on the tool's wire definition, but no native `tool_search` tool is sent alongside it. OpenAI rejects a lone `defer_loading` flag with a 400 error.

The result is that the follow-up request fails in both of these cases:
- a tool that was discovered in an earlier turn of the message history;
- a tool revealed by `load_capability`.

### Required behaviour

When a model prepares its function tools, a corpus member whose associated native tool is **not** supported by the model is handled as follows:

- **Not yet discovered** (`defer_loading=True`): it is still omitted from the request, as today.
- **Already discovered** (`defer_loading=False`): it stays on the wire as a plain function tool, with its native-tool association (`with_native`) cleared. No adapter should then derive any native-tool-specific flag from it.

Concretely, consider an `OpenAIResponsesModel('gpt-5.2')` agent with `capabilities=[ToolSearch()]` and a `defer_loading=True` tool `get_weather`. Its message history shows `get_weather` as already discovered, via a `ToolSearchCallPart` / `ToolSearchReturnPart` pair. For that agent, the `tools` list in the Responses request must:
- contain no entry with `type == 'tool_search'`;
- include the `get_weather` function tool definition **without** a `defer_loading` key.

The following flow must also complete successfully end to end on `gpt-5.2`:
1. An agent is configured with a deferred `Capability` (`id='foo'`, `defer_loading=True`) that defines a tool `bar(x: int) -> int`.
2. The model calls `load_capability`.
3. The model calls `bar`.
4. The run produces a `LoadCapabilityReturnPart`, and a `ToolReturnPart` for `bar` with content `1`.

Behaviour for models that *do* support native tool search must be unchanged. On those models, discovered and undiscovered corpus members keep their native pairing and the provider-specific `defer_loading` handling.
