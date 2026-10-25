You are working in the git repository in the current directory. Make the change described below. Work autonomously: nobody will answer questions.

## `toJSONSchema`: let `override` represent an otherwise-unrepresentable type

### Problem

With the default `unrepresentable: "throw"`, `z.toJSONSchema()` throws as soon as it meets a type that has no JSON Schema equivalent (`z.date()`, `z.bigint()`, `z.map()`, `z.symbol()`, transforms, custom types, and so on). The `override` callback cannot prevent this, because the error is raised before `override` runs.

The only way to map one such type is to set `unrepresentable: "any"`. That disables the error for every unrepresentable type. For example, to emit `z.date()` as `{ type: "string", format: "date-time" }`, you currently also have to let `z.bigint()`, `z.map()` and the rest silently become `{}`.

### Required behaviour

Under `unrepresentable: "throw"` (the default), the unrepresentable error must be raised only after `override` has run for that node. It must be suppressed only if `override` handled the node.

"Handled" means `override` changed at least one **non-annotation** key of that node's JSON Schema. These keys are annotations and do not count:

- `title`
- `description`
- `default`
- `examples`
- `deprecated`
- `readOnly`
- `writeOnly`
- `$comment`
- `id`
- `$id`

The rules that follow from this:

- **Handled node:** an override that sets `type`/`format` on a `z.date()` node produces that schema, and no error is thrown.
- **Other nodes still throw:** in the same schema, an unhandled `z.bigint()` still throws `"BigInt cannot be represented in JSON Schema"`.
- **Annotation-only overrides:** an override that only adds a `description` (or other annotation) does not suppress the error. Metadata already attached via `.describe()` or `.meta()` does not suppress it either.
- **Value-level cases keep throwing unless `override` structurally changes the node:**
  - a `.catch()` whose catch function throws: `"Dynamic catch values are not supported in JSON Schema"`;
  - `z.literal([undefined, "a"])`: ``"Literal `undefined` cannot be represented in JSON Schema"``.
- **Same contexts as before:** the behaviour must hold for nodes that are nested, reused, extracted into `$defs` (e.g. via `.meta({ id })`), wrapped (`z.optional(...)`), or cloned by `.describe()`/`.meta()`. It must also hold for every `target`, including `"openapi-3.0"`.
- **`unrepresentable: "any"`** is unchanged: such types still become `{}`.

### Error messages

Errors are plain `Error`s, and their messages must stay exactly as they are now:

- `"BigInt cannot be represented in JSON Schema"`
- `"Symbols cannot be represented in JSON Schema"`
- `"Undefined cannot be represented in JSON Schema"`
- `"Void cannot be represented in JSON Schema"`
- `"Date cannot be represented in JSON Schema"`
- ``"Literal `undefined` cannot be represented in JSON Schema"``
- `"BigInt literals cannot be represented in JSON Schema"`
- `"NaN cannot be represented in JSON Schema"`
- `"Custom types cannot be represented in JSON Schema"`
- `"Function types cannot be represented in JSON Schema"`
- `"Transforms cannot be represented in JSON Schema"`
- `"Map cannot be represented in JSON Schema"`
- `"Set cannot be represented in JSON Schema"`
- `"Dynamic catch values are not supported in JSON Schema"`

### Example

```ts
z.toJSONSchema(z.object({ when: z.date(), id: z.bigint() }), {
  override: (ctx) => {
    if (ctx.zodSchema._zod.def.type === "date") {
      ctx.jsonSchema.type = "string";
      ctx.jsonSchema.format = "date-time";
    }
  },
});
// throws "BigInt cannot be represented in JSON Schema"
// without `id`, `when` becomes { type: "string", format: "date-time" }
```
