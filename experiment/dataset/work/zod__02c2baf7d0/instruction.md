# `z.preprocess` should defer optionality to its inner schema

## Problem

The position of `.optional()` relative to `z.preprocess` currently changes whether an object key may be omitted:

```ts
const outer = z.object({ x: z.preprocess((v) => v, z.number()).optional() });
const inner = z.object({ x: z.preprocess((v) => v, z.number().optional()) });

outer.safeParse({}); // success
inner.safeParse({}); // fails — missing key is reported as an issue
```

A preprocess step is transparent to presence semantics. If the inner schema accepts a missing value, the object property should accept a missing key too. Right now the preprocess result does not report the inner schema's optionality (`_zod.optin` / `_zod.optout`).

A related JSON Schema bug: for `z.codec(...)` carrying output-side metadata such as `examples`, those examples leak into the **input** JSON schema. A codec is a transforming schema, so output-side examples and defaults must be stripped when generating with `io: "input"`, the same way they are for other transforms.

## Requirements

### New `ZodPreprocess` type
- `z.preprocess(fn, schema)` returns a `z.ZodPreprocess<U>`, where `U` is the type of `schema`.
- `z.ZodPreprocess` is exported as a class/constructor from `zod` (classic). A core counterpart `z.core.$ZodPreprocess` is exported along with the types `$ZodPreprocessDef<B>` and `$ZodPreprocessInternals<B>`.
- It is a structural subtype of pipe:
  - instances satisfy `instanceof z.ZodPipe` and `instanceof z.ZodPreprocess`;
  - `_zod.def.type === "pipe"`;
  - `def.in` is the transform built from `fn` and `def.out` is the inner schema.
- Type level:
  - a `ZodPreprocess<B>` is assignable to `z.ZodPipe<z.core.$ZodTransform, B>`, `z.core.$ZodPipe<z.core.$ZodTransform, B>`, `z.core.$ZodPreprocess` and `z.ZodPreprocess`;
  - `z.input` is `unknown`;
  - `z.output` is the output of `B`.

### Optionality
- `_zod.optin` and `_zod.optout` of a preprocess schema equal those of the inner schema, both at runtime and in the types.
  - For `z.preprocess(f, z.string().optional())`, both are `"optional"`.
  - For `z.preprocess(f, z.string())`, the type is `"optional" | undefined`.
- Both `outer` and `inner` in the example above accept `{}` and parse `{ x: 1 }` to `{ x: 1 }`.

### Values must *not* be inherited
- `_zod.values` and `_zod.propValues` stay `undefined` on a preprocess schema, even if the inner schema is a literal or enum. Preprocess widens the accepted input set.
- As a consequence, a discriminated union whose discriminator is a preprocess still fails with an "Invalid discriminated union option" error, as it does today.
- A record whose key schema is a preprocess must run `fn` on every input key. For example, with a key schema `z.preprocess(toLowerCase, z.enum(["a","b"]))`, the input `{ A: "x", B: "y" }` parses to `{ a: "x", b: "y" }`.

### JSON Schema
- Preprocess keeps today's behaviour: it uses the inner schema for both input and output.
- For a codec with `.meta({ examples: [42] })`, the two modes differ:
  - `io: "input"` produces only `{ type: "string" }` (plus `$schema`), with no examples;
  - `io: "output"` keeps `examples: [42]` along with `type: "number"`.
