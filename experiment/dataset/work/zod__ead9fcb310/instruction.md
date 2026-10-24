# Declared `__proto__` keys are lost or corrupt the prototype in `z.object()` and `z.record()` (v4)

## Problem

When a schema *explicitly declares* `"__proto__"` as a key, Zod v4 does not produce that key in the parsed output. The result is assembled into a fresh `{}`. Writing `"__proto__"` onto it by ordinary assignment triggers the inherited `Object.prototype.__proto__` setter instead of creating a property. This has two effects:

- If the value is an object, it becomes the **prototype** of the result. Its fields then appear as inherited properties (for example, `parsed.isAdmin === true`), and the declared field is not an own property.
- If the value is a primitive (for example, `z.object({ __proto__: z.string() })` parsing `{"__proto__":"hello"}`), the value is silently discarded. Parsing reports success but returns `{}`.

This affects:

- `z.object()` shapes that contain a `"__proto__"` key, in every parse mode: JIT-compiled (default), `jitless: true`, and async (`parseAsync`).
- `z.record()` with a finite key schema that includes `"__proto__"`, such as `z.record(z.enum(["__proto__", "b"]), ...)` or `z.record(z.literal(["__proto__", "b"]), ...)`, in both sync and async parsing.

Note that such shapes usually have to be built with `Object.fromEntries([["__proto__", schema], ...])`, because an object literal `{ __proto__: ... }` sets the prototype instead of creating a key. Likewise, inputs come from `JSON.parse`, which creates `__proto__` as an own key.

## Required behaviour

When `"__proto__"` is a key declared by the schema (an object shape key, or a member of a finite record key set):

- The parsed result's prototype remains `Object.prototype`.
- `"__proto__"` is an **own, enumerable, writable, configurable data property** of the result. Its value is the validated and parsed value for that key.
  - Primitives are preserved.
  - Object values are not merged into the prototype chain. For example, after parsing `{"__proto__":{"isAdmin":true},"name":"alice"}`, `parsed.isAdmin` is `undefined`.
- `Object.keys(result)` includes `"__proto__"` alongside the other declared keys.
- Validation still applies to the `__proto__` value. An invalid value makes parsing fail as usual.
- `Object.prototype` is never modified.

The JIT, jitless and async paths must all behave identically.

## Unchanged behaviour

`__proto__` keys that come only from the input, and are not declared by the schema, are still dropped as before. This covers:

- passthrough, loose and catchall objects;
- `z.record(z.string(), ...)` and other non-finite key schemas.

Keys other than `"__proto__"` continue to be assigned exactly as they are today, with no behavioural or performance change.
