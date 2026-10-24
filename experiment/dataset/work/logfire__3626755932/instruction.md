# Fix races in Claude Agent SDK tool-use hook instrumentation

## Problem

The Claude Agent SDK integration (`logfire.instrument_claude_agent_sdk`) creates `execute_tool <name>` spans from the SDK's PreToolUse/PostToolUse hooks. The SDK runs these hooks in separate anyio tasks. This causes two problems.

1. **Lost chat output.** The CLI sends the assistant message containing the `tool_use` block before it invokes the PreToolUse hook. However, the hook task can be scheduled before the main `receive_response()` loop has applied that assistant message to the conversation state. When that happens, the hook closes the current chat span too early, and the chat span loses its output messages and model attributes. This shows up whenever the consumer of `receive_response()` is slow, for example if it awaits something between messages. The spans that are produced must be the same no matter how fast the consumer is.

2. **Wrong code attributes on tool spans.** `execute_tool` spans get `code.filepath`, `code.function` and `code.lineno` from the hook task's stack. That stack is rooted in the event loop and has no meaningful user frame. Tool spans should instead have the same user code location as the chat spans: the user's frame that called `receive_response()`. For example, `code.filepath` should be the user's file name and `code.function` should be the user's function. These attributes must be ordinary code attributes and must not appear in `logfire.json_schema`.

## Requirements

The internal conversation-state class `_ConversationState` in `logfire._internal.integrations.claude_agent_sdk` is used directly by callers that drive the hooks manually. It must expose the following:

- `announced_tool_ids: set[str]`. Starts empty. When an assistant message is handled, the IDs of all `ToolUseBlock`s in its content are added to this set.
- `tool_announcement_timeout: float`. Defaults to `1.0` (seconds). It can be changed on an instance after construction.
- `async wait_for_tool_announcement(tool_use_id: str) -> None`. Returns as soon as `tool_use_id` is in `announced_tool_ids`. If the ID is not announced within `tool_announcement_timeout` seconds, it returns anyway without raising.

Required behaviour of `pre_tool_use_hook`:

- Before it closes the chat span or creates the tool span, it waits for the tool call to be announced, using the mechanism above.
- While it is waiting, no entry for that tool ID exists in `state.active_tool_spans`.
- Handling an assistant message that announces the ID wakes the waiting hook, which then creates the span.
- If the ID is never announced, the span is still created after the timeout.
- Multiple hooks may be waiting at once, and successive announcements must keep working. A waiter must only proceed once its own ID has been announced.

The code location (`code.filepath`, `code.function`, `code.lineno`) used for tool spans must be captured when the conversation state is created, which happens during the user's `receive_response()` call. That location must then be applied to every `execute_tool` span.
