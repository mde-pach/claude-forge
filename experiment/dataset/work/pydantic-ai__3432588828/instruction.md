# `TestModel` responses lack `provider_name` outside streaming mode

## Problem

`TestModel` (`pydantic_ai.models.test.TestModel`) stands in for a real LLM in tests. The metadata on the `ModelResponse` messages it produces depends on how the agent is run:

- **Streaming** (`agent.run_stream(...)`, `agent.run_stream_sync(...)`): the resulting `ModelResponse` has `provider_name='test'`.
- **Non-streaming** (`agent.run(...)`, `agent.run_sync(...)`, `pydantic_ai.direct.model_request` / `model_request_sync`): the resulting `ModelResponse` has `provider_name=None`.

Code that checks or snapshots message history therefore sees different results for the same prompt depending on run mode. Real models set `provider_name` in both modes.

## Expected behaviour

Every `ModelResponse` that `TestModel` returns from a non-streaming request must have `provider_name` set to the model's system/provider name. For a default `TestModel()` this is the string `'test'`. This is the same value streaming responses already carry.

It applies to every kind of response `TestModel` generates:

- tool-call responses;
- text responses such as `'success (no tool calls)'`;
- JSON summaries of tool results, e.g. `'{"my_ret":"1"}'`;
- structured output-tool responses;
- custom `custom_output_text` / `custom_output_args` responses.

It also applies wherever `TestModel` is the underlying model, for example inside durable-execution integrations such as Temporal.

All other response fields must stay as they are:

- `model_name='test'`
- usage estimates (`RequestUsage` input/output tokens)
- timestamp
- `provider_details`, `provider_response_id` and `provider_url` remain `None`

When a run's messages are serialised (e.g. via `ModelMessagesTypeAdapter.dump_python(..., mode='json')` or the JSON helpers on run results), each response must now contain `'provider_name': 'test'` instead of `'provider_name': None`.

## Acceptance criteria

Given `agent = Agent(model=TestModel())`, these two calls must yield identical lists of `ModelResponse` objects from `all_messages()`, apart from run/conversation IDs and timestamps:

- `agent.run_sync('hello')`
- `agent.run_stream_sync('hello')`, with its text fully consumed

Each list must contain exactly one response with these values:

| Field | Value |
|---|---|
| `parts` | `[TextPart(content='success (no tool calls)')]` |
| `usage` | `RequestUsage(input_tokens=51, output_tokens=4)` |
| `model_name` | `'test'` |
| `provider_name` | `'test'` |
