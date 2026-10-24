# `new_messages()` leaks the resumed request when it is rebuilt by a capability or history processor

## Problem

You can run an agent with `message_history` whose last message is a `ModelRequest` and pass no new user prompt (`agent.run(message_history=[...])`). In that case, the trailing request is prior context. It is not a new message. `result.new_messages()` must exclude it, even though the framework stamps it with the current `run_id`.

Today that exclusion only works when the resumed request reaches the model unchanged, either as the same object or as an equal value. If a capability or history processor rebuilds that request with any changed field, detection falls back to `run_id`. The rebuilt request then leaks into `new_messages()`. Examples of such rebuilds:

- `ReinjectSystemPrompt(replace_existing=True)` (from `pydantic_ai.capabilities`), which prepends a `SystemPromptPart` to the first request. This is the UI adapters' default with server-managed system prompts.
- A `ProcessHistory(...)` processor that rebuilds requests with `dataclasses.replace(...)`, for example by changing `metadata`.
- A processor that replaces the resumed request with completely different content.

## Required behaviour

When resuming without a user prompt, `result.new_messages()` must return only the messages produced by the current run. `capture_run_messages()` must still equal `result.all_messages()`. Specifically:

1. **In-place rebuild.** If the resumed (trailing) request is rebuilt in place with different parts or fields, it is still excluded. For a single model call, `new_messages() == all_messages()[-1:]`. This applies to `ReinjectSystemPrompt(replace_existing=True)` with an agent `system_prompt`, to metadata-changing processors, and to processors that fully replace the request.
2. **Insert and rebuild together.** A processor may insert messages ahead of the resumed request and also rebuild it in the same pass. Neither the inserted message nor the rebuilt resumed request appears in `new_messages()`.
3. **Multi-step runs, such as a tool-call loop.** If the resumed request is rebuilt on every step (for example by system-prompt reinjection), it stays excluded across all steps. Every message produced during the run is included. For a history starting with just the resumed request, `new_messages() == all_messages()[1:]`.
4. **Resumed request removed later.** A processor may drop the resumed request partway through the run, for example "keep last N". In that case, no message produced earlier in the run is excluded, and `new_messages()` returns every surviving message.
5. **Message removed after the resumed request.** A processor may remove a message that sits after the resumed request on a later step, for example dropping the earlier tool-call response once the tool result is present. The resumed request, if unchanged, is still excluded, and only the messages after it are new.
6. **No regressions.** Existing behaviour for identity- and value-based matching must continue to work. This includes reordering, deep-copying, and insertion or removal of other messages. When the resumed request cannot be located at all, the fallback is still `run_id`-based detection.

No new public API is introduced. The fix concerns the observable results of `new_messages()` and `all_messages()` from `agent.run(...)` and streaming runs, including runs where a capability raises `SkipModelRequest`.
