# `toJSONSchema`: accept a function for `unrepresentable`

## Problem

`z.toJSONSchema` handles types with no JSON Schema equivalent (`date`, `bigint`, `map`, and so on) in one of two ways, and neither is selective:

- `"throw"` throws for every such type.
- `"any"` turns every such type into `{}`.

A caller who wants, for example, `z.date()` to become `{ type: "string", format: "date-time" }` while everything else stays strict has no clean option. The current behaviour also leaks. It defers the unrepresentable error until after `override` runs, then guesses from what `override` changed whether the type was "handled". As a result, an unrelated `override` (such as one setting OpenAPI `nullable`) can swallow the error and emit a schema with no type.

## Required behaviour

The `unrepresentable` option of `z.toJSONSchema` (and of `ToJSONSchemaParams` / `JSONSchemaGeneratorParams`) must also accept a function:

```ts
unrepresentable?: "throw" | "any" | UnrepresentableHandler<$ZodTypes>
```

Export the handler type from core as `z.core.UnrepresentableHandler<T extends $ZodType = $ZodType>`:

```ts
(ctx: { zodSchema: T; path: (string | number)[]; message: string })
  => JSONSchema.BaseSchema | "throw" | "any" | undefined
```

### Context passed to the handler

- `zodSchema` is the schema at the unrepresentable site.
- `path` is the JSON Schema path to that site, for example `["properties", "when"]`.
- `message` is the exact error Zod would otherwise throw, for example `"BigInt cannot be represented in JSON Schema"`.

### When the handler is called

The handler is called once per unrepresentable site, for all of these:

- `bigint`, `symbol`, `undefined`, `void`, `date`, `nan`
- custom, map, set, transform, function
- each `undefined` member of a literal and each `bigint` member of a literal
- a dynamic `.catch()` value (one whose catch function throws)

Literal members share the same `zodSchema`. Only `message` distinguishes them:

- `"Literal \`undefined\` cannot be represented in JSON Schema"`
- `"BigInt literals cannot be represented in JSON Schema"`

The handler is called in member order.

### Return values

- **A JSON Schema object:** it is used for that node.
  - For a literal, it replaces the whole literal output. No `enum`/`const` is emitted and no further members are processed.
  - For a dynamic `.catch()`, it is merged into the node, so the inner type is kept. For example, returning `{ default: "fallback" }` on `z.string().catch(...)` gives `{ type: "string", default: "fallback" }`.
  - The result must survive `$defs` extraction.
- **`"any"`:** identical to the existing `unrepresentable: "any"` behaviour at that site. For literals, `undefined` members are dropped and `bigint` members are coerced to numbers.
- **`"throw"` or `undefined`:** throw an `Error` with the site's default message.
- **Errors thrown by the handler** propagate unchanged.

### Interaction with `override`

- The handler runs before `override`. `override` sees the JSON Schema the handler produced.
- `override` no longer affects whether an unrepresentable error is thrown. Under `"throw"`, the error is thrown at the site, whatever `override` would do.

### Unchanged behaviour

- The string options `"throw"` (the default) and `"any"` behave as before.
- Output for `target: "openapi-3.0"` must remain valid OpenAPI 3.0.

## Example

```ts
z.toJSONSchema(z.object({ createdAt: z.date(), id: z.bigint() }), {
  unrepresentable: ({ zodSchema }) =>
    zodSchema._zod.def.type === "date" ? { type: "string", format: "date-time" } : "throw",
});
// throws "BigInt cannot be represented in JSON Schema"
// without `id`, createdAt => { type: "string", format: "date-time" }
```
