## Multimodal tool-return content is not rehydrated by `validate_python`

`ToolReturnPart.content` may hold `MultiModalContent` (`ImageUrl`, `AudioUrl`, `VideoUrl`, `DocumentUrl`, `BinaryContent`, `UploadedFile`). It may be a scalar, or nested inside lists and dicts.

When message history is serialized and loaded again, these items should come back as the original classes. That works for `ModelMessagesTypeAdapter.validate_json(...)`. It does not work for `ModelMessagesTypeAdapter.validate_python(...)` on data produced by `dump_python(messages, mode='json')`. That is the replay path used by callers that have already parsed the JSON. On that path, a dumped multimodal item such as `{'kind': 'binary', 'data': '...', 'media_type': 'audio/mpeg', ...}` comes back as a plain `dict` instead of a `BinaryContent`.

### Required behaviour

The `ToolReturnContent` type, importable from `pydantic_ai.messages`, must meet the following requirements. They apply to both `validate_json(dump_json(...))` and `validate_python(dump_python(..., mode='json'))`.

1. **Rehydration.** Every type in `MULTI_MODAL_CONTENT_TYPES` used as tool-return content must load back as an instance of exactly that class, and compare equal to the original. This applies when the item is:
   - the scalar content;
   - an element of a list, e.g. `['hello', BinaryContent(...)]`;
   - a value in a dict, e.g. `{'caption': '...', 'attachment': BinaryContent(...)}`.

   A `BinaryContent` with a non-image media type must remain `BinaryContent`.

2. **User dicts that reuse a `kind` value stay plain mappings.** A dict counts as multimodal only if it meets both conditions:
   - its `kind` is a string matching one of the multimodal kinds;
   - it carries at least one type-specific field: `url`, `media_type` or `file_id`.

   For example, `{'kind': 'binary', 'label': 'foo'}` or `{'kind': 'image-url', 'note': '...'}` must round-trip unchanged as a dict. It must not raise.

3. **Invalid lookalikes fall back gracefully.** A dict that has a multimodal `kind` and a type field but is not a valid instance must round-trip unchanged as a plain dict. It must not raise a `ValidationError`. Examples:
   - `{'kind': 'binary', 'media_type': 'text/plain', 'text': 'hello'}` (no `data`);
   - `{'kind': 'uploaded-file', 'file_id': 'abc', 'status': 'ready'}` (no provider);
   - `{'kind': 'image-url', 'media_type': 'image/png', 'note': 'x'}` (no `url`).

   Serializing such content must not emit any warnings, including `PydanticSerializationUnexpectedValue`. The full round trip must succeed with warnings turned into errors.

4. **Malformed `kind` values.** A dict whose `kind` is unhashable or not a string (a list, a dict or a `bytearray`) must not cause a `TypeError`. When validated through `ModelMessagesTypeAdapter.validate_python`, it must be kept as a plain mapping. This applies both at top level and nested inside a list.

5. **No regressions.** Other content must keep round-tripping as before. This covers strings, bytes, plain lists and dicts, and the current list structure.
