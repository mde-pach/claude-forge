## `z.record` lets a key schema turn an input key into `__proto__`, polluting the parsed result's prototype

### Problem

`z.record(keySchema, valueSchema)` already ignores an input key that is literally `"__proto__"`. That check runs on the **raw** input key, but the output is written under the key the **key schema produces**. When the key schema changes the key, an ordinary-looking key can become `"__proto__"` after parsing. Writing to that key goes through the inherited `__proto__` setter, so the parsed result's prototype is replaced with data the attacker supplied.

Example:

```ts
const schema = z.record(z.string().toLowerCase(), z.object({ a: z.string() }));
const result = schema.parse(JSON.parse('{"__PROTO__": {"a": "evil"}}'));
Object.getPrototypeOf(result); // currently { a: "evil" } — should be Object.prototype
result.a;                      // currently "evil" — should be undefined
```

Any mechanism that changes the key can trigger this. Examples:
- `.toLowerCase()` with `"__PROTO__"`
- `.trim()` with `" __proto__ "`
- `.normalize("NFKC")` with fullwidth `"＿＿ｐｒｏｔｏ＿＿"`
- `.transform(...)`, `.overwrite(...)`, `z.preprocess`, `.pipe(...)` and codecs

The problem affects both `parse` and `parseAsync`, including the async path that runs when the value schema has async refinements. It also affects both the classic (`zod`) and mini (`zod/mini`) APIs.

### Required behaviour

- If the key schema's output for an entry is exactly `"__proto__"`, the record parser must drop that entry silently, the same way it already handles a raw `"__proto__"` input key. Specifically:
  - No issue is raised.
  - No value is written.
  - The result's prototype stays `Object.prototype`.
  - `__proto__` does not appear in `Object.keys(result)`.
- This must work for:
  - sync parsing (`parse`, `safeParse`)
  - async parsing (`parseAsync`, `safeParseAsync`)
  - value schemas that are themselves async, such as `z.object(...).refine(async () => true)`
- Normal key normalization must keep working. For example, `z.record(z.string().toLowerCase(), v).parse({ KEY: x })` must still return `{ key: x }`, with `key` as an own property.
- The existing handling of a raw `"__proto__"` input key stays as it is.
- Explicitly declared `__proto__` keys are out of scope for this change.
