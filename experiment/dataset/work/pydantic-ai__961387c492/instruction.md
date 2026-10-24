## `FilePart` file content loses `vendor_metadata` in AG-UI and Vercel AI message round-trips

### Problem

A `FilePart` in a `ModelResponse` holds a `BinaryContent` (or a subclass such as `BinaryImage`). That content can carry `vendor_metadata`, for example `{'detail': 'high'}` or video offsets like `{'fps': 24, 'start_offset': '12.5s'}`.

Two adapters drop this metadata when they convert messages to their UI format and back:

- `AGUIAdapter.dump_messages` / `AGUIAdapter.load_messages`
- `VercelAIAdapter.dump_messages` / `VercelAIAdapter.load_messages`

After a round-trip, `file_part.content.vendor_metadata` is `None`. The `id`, `provider_name` and `provider_details` fields of the part already survive.

### Requirements

**AG-UI adapter**

- In `AGUIAdapter.dump_messages(messages, preserve_file_data=True)`, a `FilePart` becomes an `ActivityMessage` with `activity_type == 'pydantic_ai_file'`.
  - If `part.content.vendor_metadata` is not `None`, the message's `content` dict must contain a `'vendor_metadata'` key holding that dict unchanged.
  - If it is `None`, the `'vendor_metadata'` key must be absent from `content`.
- `AGUIAdapter.load_messages(..., preserve_file_data=True)` must restore the value. When a `pydantic_ai_file` activity message's `content` has a dict under `'vendor_metadata'`, the resulting `FilePart.content.vendor_metadata` must equal it.
- A full dump/load round-trip must reproduce the original messages exactly, including `vendor_metadata`.
- The `FileActivityContent` typed dict must gain an optional `vendor_metadata: dict[str, Any]` key.
- In `UploadedFileActivityContent`, `vendor_metadata` must be typed `dict[str, Any]` instead of `Any`.

**Vercel AI adapter**

- In `VercelAIAdapter.dump_messages`, each `FilePart` becomes a `FileUIPart`. Its `provider_metadata` must carry the content's `vendor_metadata` next to the existing `id`, `provider_name` and `provider_details`.
  - Reading the field back with `load_provider_metadata(file_ui_part.provider_metadata)` must return a mapping whose `'vendor_metadata'` entry equals the original dict.
  - This means the existing `dump_provider_metadata` helper needs to accept a `vendor_metadata` keyword.
- `VercelAIAdapter.load_messages` must restore that value onto the reloaded `FilePart.content.vendor_metadata`.

**Validation of client-supplied values (both adapters)**

The client controls this data, and assigning to `BinaryContent` does not validate it. On load, `vendor_metadata` must be applied only when it is a dict with string keys. Anything else, such as the string `'not-a-dict'`, must be silently ignored:

- no error is raised;
- the reloaded `FilePart.content.vendor_metadata` is `None`.
