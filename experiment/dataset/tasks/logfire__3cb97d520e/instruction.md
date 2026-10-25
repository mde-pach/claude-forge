You are working in the git repository in the current directory. Make the change described below. Work autonomously: nobody will answer questions.

# Support OpenAI Responses API content parts (text, image, file) in instrumentation

## Problem

The OpenAI instrumentation handles message content parts from the Chat Completions API (`text`, `output_text`, `image_url`). It does not understand the content parts used by the **Responses API**: `input_text`, `input_image` and `input_file`. As a result, user inputs that use these parts are not converted properly in either output path:

- the semantic-convention message conversion (`gen_ai.input.messages`);
- the legacy events path.

## Requirements

### New message part type

Add a `FilePart` TypedDict to `logfire._internal.integrations.llm_providers.semconv` for references to files uploaded to the provider:

- `type: Literal['file']`
- `file_id: str`
- optional `media_type: str`
- optional `modality: Literal['image', 'audio', 'video', 'document']`

Include `FilePart` in the `MessagePart` union.

### `convert_responses_inputs_to_semconv(inputs, instructions)`

This function lives in `logfire._internal.integrations.llm_providers.openai`. Content parts must be converted as follows. For each kind of part, use the first rule that matches.

- **Text:** `input_text`, `text` and `output_text` become `{'type': 'text', 'content': <text>}`.
- **Images:** `image_url` and `input_image`:
  1. A URL becomes `{'type': 'uri', 'uri': ..., 'modality': 'image'}`. The URL is read from the `image_url` key, which may be either a string or a dict with a string `url`.
  2. Otherwise, a truthy `file_data` becomes `{'type': 'blob', 'content': <file_data>, 'modality': 'image'}`.
  3. Otherwise, a string `file_id` becomes `{'type': 'file', 'file_id': ..., 'modality': 'image'}`.
  4. Otherwise, the original part dict is returned unchanged.
- **Files:** `input_file` follows the same rules as images, with two differences:
  - the URL comes from `file_url`, which may be a string or a dict with `url`;
  - the modality is `'document'`.

  Other keys such as `filename` are not included in the converted part. A part matching none of the rules is returned unchanged.

### `input_to_events(inp, tool_call_id_to_name)`

This function is in the same module. For message content lists, each part produces one event of the form `{'event.name': <event name>, 'content': <value>, 'role': <role>}`. The `content` value is chosen by the first matching rule:

- **Text:** `input_text` and `output_text` use the text.
- **Images:** `image_url` and `input_image`:
  1. the URL, resolved as above;
  2. otherwise, if `file_data` is present and not `None`, the literal string `'[file_data]'`;
  3. otherwise, the `file_id`;
  4. otherwise, the existing unknown event (`gen_ai.unknown`).
- **Files:** `input_file`:
  1. the `file_url`;
  2. otherwise, the `file_id`;
  3. otherwise, a string `filename`;
  4. otherwise, `'[file_data]'` if `file_data` is present;
  5. otherwise, the literal `'[file]'`.
- **Any other part type:** still produces the existing unknown event:

  ```python
  {
      'event.name': 'gen_ai.unknown',
      'role': 'unknown',
      'content': '<type>\n\nSee JSON for details',
      'data': <part>,
  }
  ```
