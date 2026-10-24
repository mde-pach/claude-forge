## `.catch()` fields in objects reject missing keys

### Problem

Object parsing in Zod v4 rejects an absent key unless that key's schema is treated as optional on input. A schema wrapped in `.catch(...)` currently takes its input optionality from the inner schema. So when the inner schema is not optional, a missing key fails with an `invalid_type` / `"nonoptional"` issue, and the catch handler never runs.

Examples that fail on `{}` but should succeed:

```ts
z.object({ items: z.preprocess(fn, z.array(z.string())).catch([]) }).parse({});
z.object({ fruit: z.enum(["apple", "orange"]).catch("apple") }).parse({});
z.object({ fruit: z.nativeEnum(Fruits).catch(Fruits.apple) }).parse({});
```

This is a regression from v4.3.x. A catch schema always handles `undefined` input: its handler runs and produces the fallback value.

### Required behaviour

1. **Catch schemas are optional on input.** Any `X.catch(...)` schema reports input optionality as optional (`_zod.optin === "optional"`), whatever `X` is.

2. **Absent keys run the catch handler.** In both normal and `{ jitless: true }` modes:
   - `z.object({ fruit: z.enum(["apple","orange"]).catch("apple") }).parse({})` returns `{ fruit: "apple" }`.
   - The `z.nativeEnum` equivalent returns `{ fruit: Fruits.apple }`.
   - Present but invalid values still fall back to the catch value, as they do today.

3. **`.optional()` around `.catch()` keeps current semantics.** For `X.catch(v).optional()`:
   - If the original input is `undefined`, or the key is absent, the result is `undefined` and the catch value is not used.
   - In an object, an absent key with this schema is omitted from the output.
   - A present but invalid input still yields the catch value.
   - This must work for both sync and async parsing.

   Example: this object parsed with `{}`
   - `a: z.string().catch("catch value").optional()`
   - `d: z.string().catch("catch value")`
   - plus default/prefault fields

   produces the following, with no `a` key and no issues:

   ```
   { b: "default value", c: "prefault value", d: "catch value", e: "default value", f: "prefault value" }
   ```

4. **Plain optional schemas are unaffected.** `z.string().optional()` and similar schemas behave exactly as before.

5. **JSON Schema input mode.** When generating JSON Schema with `io: "input"`, an object property whose schema is a `.catch(...)` wrapper is no longer listed in `required`. Previously it was required when its inner schema was required.
