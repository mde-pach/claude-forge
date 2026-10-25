You are working in the git repository in the current directory. Make the change described below. Work autonomously: nobody will answer questions.

## Instrumented model requests crash when tool definitions contain lone surrogates

### Problem

When a model is wrapped with `InstrumentedModel` (from `pydantic_ai.models.instrumented`), each model request opens a span and records two JSON attributes. These are recorded regardless of the `include_content` setting:

- `gen_ai.tool.definitions`: the tool definitions sent to the model.
- `model_request_parameters`: the serialized `ModelRequestParameters`.

Text can contain unpaired (lone) surrogate code points, for example a tool description decoded with `errors='surrogateescape'` such as `'get the weather before\udce4after'`. If such text appears in a tool definition or anywhere else in the request parameters, building these attributes raises `PydanticSerializationError`. An otherwise successful agent run then crashes because of instrumentation.

Other instrumentation attributes already go through the library's surrogate-tolerant JSON serialization. These two attributes do not.

### Required behaviour

- Calling `request(...)` on an `InstrumentedModel` must not raise when any `ToolDefinition` in `ModelRequestParameters(function_tools=[...])` contains lone surrogates in its name, description or parameters schema. The same applies to lone surrogates anywhere in the request parameters.
- The `gen_ai.tool.definitions` span attribute must still be a JSON string. Parsed, it is a list of objects shaped like:
  ```json
  {"type": "function", "name": "weather", "description": "get the weather before\udce4after", "parameters": {"type": "object", "properties": {}}}
  ```
  The lone surrogate must be preserved: once the attribute is parsed back from JSON, the description equals the original Python string.
- The `model_request_parameters` span attribute must still be a JSON string. Parsed, it contains the full request parameters, including `function_tools[i].name`, with the same surrogate-tolerant handling.
- Output for ordinary text, with no surrogates, must be unchanged.
- `safe_to_json` in `pydantic_ai._instrumentation` keeps its existing behaviour: it returns compact JSON `bytes` and tolerates lone surrogates. It must accept any `object`.
