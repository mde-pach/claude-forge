# `z.preprocess` no longer narrows the inferred input type from an annotated preprocessor argument

## Problem

When the preprocessor function passed to `z.preprocess` has a type annotation on its argument, that type should become the schema's input type. It currently does not. `z.input` always resolves to `unknown`, whatever the annotation says:

```ts
const trimmed = z.preprocess((val: string | null | undefined) => val?.trim() ?? "", z.string());
type In = z.input<typeof trimmed>; // currently `unknown`, expected `string | null | undefined`
```

The same loss happens when a preprocessed schema is nested inside other schemas. For example, the input type of `z.object({ a: z.preprocess((v: string | number) => String(v), z.string()) })` should be `{ a: string | number }`.

This is a type-level problem only. Runtime behaviour must not change.

## Requirements

### Generic parameters

- `z.ZodPreprocess` (classic) takes a second, optional generic parameter for the preprocessor's argument type: `ZodPreprocess<B extends SomeType = $ZodType, I = unknown>`.
- The core types take the same second parameter, also defaulting to `unknown`:
  - `z.core.$ZodPreprocess<B, I>`
  - `z.core.$ZodPreprocessDef<B, I>`
  - `z.core.$ZodPreprocessInternals<B, I>`
- In all of these, the `in` transform is typed with input `I`.

### Return type of `z.preprocess`

- `z.preprocess(fn, schema)` returns `ZodPreprocess<typeof schema, ArgType>`.
- `ArgType` is the type of `fn`'s first parameter.
- If `fn`'s argument is not annotated, `ArgType` stays `unknown`, so `z.input` remains `unknown` exactly as it is today.

### Inferred types

- `z.input<typeof pre>` equals the annotated argument type.
- `z.output<typeof pre>` remains the output of the target schema. For example, it is `string` for `z.string()`.
- Both hold when the schema is nested, for example inside `z.object`.

### Assignability

A narrowed preprocess schema must still be assignable to the bare, un-parameterised types. Both of these must compile:

```ts
const pre = z.preprocess((v: string) => v.length, z.number());
const a: z.ZodPreprocess<z.ZodNumber> = pre;
const b: z.core.$ZodPreprocess = pre;
```

For this reason, the narrowing must apply only to the transform's input. The transform's output type should remain `unknown`.

### Optionality

`_zod.optin` and `_zod.optout` continue to be derived from the target schema. For `z.number()`, the target's `optin` is `"optional" | undefined`.
