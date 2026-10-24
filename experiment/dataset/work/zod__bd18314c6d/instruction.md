## `toJSONSchema` produces invalid `$ref` pointers when a schema id contains `/` or `~`

### Problem

When a schema is registered with an `id` (e.g. via `.meta({ id: ... })`) and `z.toJSONSchema` extracts it into the definitions section, the generated `$ref` is built by appending the raw id to the definitions path. A `$ref` fragment is a JSON Pointer (RFC 6901), in which `/` separates reference tokens and `~` is an escape character. An id containing either character therefore produces a broken pointer.

For example:

```ts
const User = z.object({ name: z.string() }).meta({ id: "Shared/User~" });
z.toJSONSchema(z.object({ User }));
```

This currently yields `"$ref": "#/$defs/Shared/User~"`. A JSON Schema validator reads that as the path `$defs` → `Shared` → `User~`, which does not resolve to the extracted definition.

### Required behaviour

- Whenever `toJSONSchema` emits a `$ref` that points at an extracted definition, the definition id inside the pointer must be escaped according to RFC 6901:
  - `~` becomes `~0`
  - `/` becomes `~1`
  - `~` must be escaped before `/`, so that a literal `~1` in an id becomes `~01` and is not misread.
- The **key** under the definitions object (`$defs`, or `definitions` for draft targets that use it) must remain the **original, unescaped** id.
- This applies to local refs (`#/$defs/<id>`) and also to refs into the shared definitions document generated when converting a registry with a `uri` option (`<uri("__shared")>#/$defs/<id>`).
- Ids without `/` or `~`, including auto-generated ids such as `__schema0` and `schema0`, must produce exactly the same output as before.

### Example of expected output

For the example above:

- `result.properties.User.$ref` must equal `"#/$defs/Shared~1User~0"`
- `Object.keys(result.$defs)` must equal `["Shared/User~"]`
