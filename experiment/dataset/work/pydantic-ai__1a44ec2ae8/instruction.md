## AG-UI: `dump_messages` reorders parts within a `ModelRequest`

### Problem

`AGUIAdapter.dump_messages(...)` (from `pydantic_ai.ui.ag_ui`) converts pydantic-ai message history into AG-UI messages. Within a single `ModelRequest`, all user content is currently collected and emitted as one `UserMessage` placed *before* every tool message from that request. The original order of the request's parts is lost.

Example: a `ModelRequest` with these parts, in order:

1. `UserPromptPart('Before the tool return.')`
2. `ToolReturnPart(tool_call_id='call_1', content='suggested')`
3. `UserPromptPart('After the tool return.')`

This is currently dumped as one combined `UserMessage` followed by the `ToolMessage`.

This breaks real conversations. A `ToolReturnPart` that responds to a preceding assistant tool call can end up separated from that call by user content. Providers such as Anthropic require each `tool_use` block to be followed immediately by its `tool_result`, so they reject the next request.

A related case: an `UploadedFile` inside a `UserPromptPart` is emitted as its own `ActivityMessage` (when `preserve_file_data=True`). Text that came before the file is currently moved after it.

### Required behaviour

When a `ModelRequest` is dumped, the output must keep the original relative order of user content, tool messages and uploaded-file activity messages.

- Consecutive user content (text and multimodal items) is still grouped into a single `UserMessage`.
- That group ends whenever one of these is emitted:
  - a `ToolMessage`, from a `ToolReturnPart` or from a `RetryPromptPart` that has a `tool_name`;
  - an `ActivityMessage` for an `UploadedFile`.
- The group is emitted at its original position, before the message that ended it.
- User content after such a message starts a new `UserMessage`.
- Each emitted `UserMessage` follows the existing simplification rule: if it holds exactly one text item, `content` is a plain string; otherwise it is a list of content items.
- System prompt handling is unchanged. All `SystemPromptPart`s are still combined into one `SystemMessage` emitted first for that request.

### Expected results

For the example above, preceded by a `ModelResponse` containing the matching `ToolCallPart`, the dumped message types are:

`AssistantMessage, UserMessage, ToolMessage, UserMessage`

The contents are, in order:

- `'Before the tool return.'`
- `'suggested'` (the `ToolMessage`, with `tool_call_id == 'call_1'`)
- `'After the tool return.'`

For a single `UserPromptPart` with content `['Before the file.', UploadedFile(file_id='file-abc123', provider_name='anthropic'), 'After the file.']`, dumped with `preserve_file_data=True`, the result is:

`UserMessage('Before the file.'), ActivityMessage(content['file_id'] == 'file-abc123'), UserMessage('After the file.')`
