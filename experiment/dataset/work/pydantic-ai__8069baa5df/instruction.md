## Bedrock: tool results with an empty list produce an empty `content` array

When a tool returns an empty list (for example, `ToolReturnPart(tool_name='lookup', content=[], tool_call_id='id1')`), `BedrockConverseModel` turns it into a Bedrock `toolResult` block whose `content` is an empty array:

```python
{'toolResult': {'toolUseId': 'id1', 'content': [], 'status': 'success'}}
```

The Bedrock Converse API requires tool result content to be non-empty, so the request fails. This is a normal tool outcome, such as a search with no matches, and it should not break the conversation.

### Required behaviour

When `BedrockConverseModel` builds the Converse message for a tool return, the `toolResult.content` list must never be empty. If no content blocks come out of the tool's return value, add exactly one fallback block. Its shape depends on the tool-result content mode the model already uses for the given model ID:

- **Text ("str") mode**, e.g. `us.amazon.nova-micro-v1:0`: the fallback is a text block holding the string form of the original content. For `content=[]` this gives:

  ```python
  {'toolResult': {'toolUseId': 'id1', 'content': [{'text': '[]'}], 'status': 'success'}}
  ```

- **JSON mode**, e.g. `mistral.mistral-7b-instruct-v0:2`: the fallback is a JSON block holding the original content unchanged. For `content=[]` this gives:

  ```python
  {'toolResult': {'toolUseId': 'id1', 'content': [{'json': []}], 'status': 'success'}}
  ```

In both cases the tool result sits in a `user` role message, as before:

```python
[{'role': 'user', 'content': [{'toolResult': {...}}]}]
```

`toolUseId` and `status` are filled in exactly as they are today.

### Must not change

- Tool returns that already produce at least one content block are mapped exactly as now. This covers non-empty lists, strings, dicts and multimodal content.
- No new public API, parameters or settings are introduced. The fix lives entirely in how `BedrockConverseModel` maps messages to the Converse request format.
