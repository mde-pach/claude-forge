# Narrow image `BinaryContent` to `BinaryImage` wherever `MultiModalContent` is validated

## Problem

When a `FilePart` is validated, for example when it is loaded through `ModelMessagesTypeAdapter`, an image `BinaryContent` in `FilePart.content` is turned into a `BinaryImage`. Other places that hold multi-modal content do not do this.

Image binary content inside any of these comes back as a plain `BinaryContent`, not a `BinaryImage`:

- `UserPromptPart.content`
- `ToolReturnPart.content`
- any other field typed with the `MultiModalContent` union from `pydantic_ai.messages`

As a result, message history that round-trips through serialisation produces inconsistent types. Examples of such round-trips include persisting messages as JSON or passing them across a Temporal workflow boundary. Whether an image comes back as `BinaryImage` depends on which part it lived in.

## Required behaviour

- **Narrow on every validation.** Whenever a value is validated against the `MultiModalContent` union, a `BinaryContent` whose media type is an image must become a `BinaryImage`.
  - This applies in both JSON mode (`validate_json`) and Python mode (`validate_python` on JSON-mode dumped data).
  - It must happen however the union is reached: via `UserPromptPart` content lists, `ToolReturnPart` content, `FilePart.content`, and so on.
- **Preserve data.** The narrowed `BinaryImage` must keep the same `data`, `media_type` and identifier as the original.
- **Leave non-image content alone.** `BinaryContent` with a non-image media type (for example `audio/mpeg`) must stay exactly `BinaryContent` after validation, not a subclass.
- **Keep `FilePart` unchanged.** `FilePart.content` must continue to narrow image content to `BinaryImage` as before. The narrowing logic should come from a single place shared by `FilePart` and `MultiModalContent`.
- **Keep the union inspectable.** `MultiModalContent` must remain an `Annotated` discriminated union (discriminator `'kind'`). Its members may be wrapped in `Annotated[...]` to carry validators.
  - After unwrapping any such `Annotated` member to its underlying type, the set of union members must still equal `set(MULTI_MODAL_CONTENT_TYPES)`.
  - `is_multi_modal_content` must keep working for every member type.

## Example

```python
image = BinaryContent(data=b'\x89PNG', media_type='image/png')
audio = BinaryContent(data=b'\x00\x01', media_type='audio/mpeg')
msgs = [ModelRequest(parts=[UserPromptPart(content=[image, audio])])]
loaded = ModelMessagesTypeAdapter.validate_json(ModelMessagesTypeAdapter.dump_json(msgs))
img, aud = loaded[0].parts[0].content
assert type(img) is BinaryImage
assert type(aud) is BinaryContent
```

The same holds for Temporal workflows. An image `BinaryContent` in user prompt or tool return content that crosses the workflow serialisation boundary must now come back as `BinaryImage`, with its identifier preserved.
