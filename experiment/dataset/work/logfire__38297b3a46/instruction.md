# Google GenAI span events are dropped when the event body contains a circular reference

## Problem

The Google GenAI integration records each Gemini log record as an event on the current span. The event's `event_body` attribute holds the record body serialised as JSON. The logger that does this is `SpanEventLogger` in `logfire._internal.integrations.google_genai`.

Sometimes the body contains a dict or list that refers back to itself. This can happen with upstream-captured Gemini SDK objects, such as an uploaded `File` whose dict form has self-references. When it does, JSON serialisation fails with `ValueError: Circular reference detected`. The internal error handler swallows the error, so the whole span event is silently lost.

## Required behaviour

`SpanEventLogger.emit(record)` must still add the event to the current span when the body contains circular references. The event keeps its name, timestamp and an `event_body` attribute holding valid JSON.

The existing body handling must not change. This includes the transformation of `content` parts and the `role` defaulting.

**Bodies without cycles** must serialise exactly as they do today.

**Bodies with cycles** must be serialised as follows:
- Walk the dicts, lists and tuples recursively.
- A dict, list or tuple that is already an ancestor on the current path from the root is a back-edge. Replace each back-edge with a string representation of that object. The exact text of that string is not specified.
- Expand everything else normally. Tuples serialise as JSON arrays.
- Detect cycles along the ancestry path only, not globally. An object that appears in several sibling positions without forming a cycle must be expanded in full at each position.

### Examples

**Self-referencing dict as content.** Take `body = {'content': f, 'role': 'user'}` where:

```python
f = {'name': 'files/abc123', 'mime_type': 'audio/wav'}
f['self'] = f
```

The `content` transformation copies `f` into a new dict, so `f` itself first appears one level down. The resulting `event_body` is:

```json
{"content": {"name": "files/abc123", "mime_type": "audio/wav",
             "self": {"name": "files/abc123", "mime_type": "audio/wav",
                      "self": "<some string>"}},
 "role": "user"}
```

**Self-referencing list nested in a dict.** Take `lst = [1, 2, 3]` and then run `lst.append(lst)`. A body of `{'content': {'data': lst, 'text': 'hello'}, 'role': 'user'}` serialises to:

```json
{"content": {"data": [1, 2, 3, "<some string>"], "text": "hello"}, "role": "user"}
```

When several such records are emitted inside the same span, each must appear as its own event on that span.
