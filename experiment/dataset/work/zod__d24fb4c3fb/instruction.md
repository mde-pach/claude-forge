# Consistently strip `__proto__` from parsed objects

## Problem

Zod's handling of a `__proto__` key in parsed output is inconsistent. In v4, if a schema *declares* `__proto__` (e.g. a `z.object` shape built via `Object.fromEntries`, a JSON Schema converted with `fromJSONSchema`, or a `z.record`/`z.partialRecord` whose key schema is `z.enum(["__proto__", ...])`/`z.literal("__proto__")`), the parsed result currently contains an own `__proto__` data property, the value is validated, and missing values may fail or receive defaults. Undeclared `__proto__` keys, by contrast, are dropped. Separately, in both v3 and v4, `z.intersection` merges object results with a spread, so when one operand passes the input through unchanged (e.g. `z.unknown()`, `z.any()`), an own `__proto__` from input such as `JSON.parse('{"__proto__":{...}}')` ends up in the output.

## Required behaviour

Parsed output objects must **never** contain an own `__proto__` property, and their prototype must always be `Object.prototype`. Specifically:

- **`z.object` (v4)** — a `__proto__` key declared in the shape is still kept in `.shape` (and survives `.pick`), but is ignored during parsing in every mode (JIT, `jitless: true`, async):
  - it is not written to the output;
  - its schema is not run, so invalid values (e.g. `123` for `z.string()`) do not fail, missing required values do not fail, and defaults are not applied;
  - an own getter for `__proto__` on the input is not invoked.
  - `.strict()` must still accept input containing a declared `__proto__` (it is not reported as an unrecognized key), while stripping it from the output.
- **`fromJSONSchema`** — a JSON Schema property named `__proto__` (even if required) still appears in the resulting object's `shape`, but parsing strips it: e.g. `{ role: "x", __proto__: "wrong" }` and `{ role: "x" }` both parse to `{ role: "x" }`.
- **`z.record` / `z.partialRecord` (v4, classic and mini)** — with a finite key set containing `__proto__`, that key is skipped whether present or missing (no "missing key" error, no value validation, no key transforms applied — e.g. a `z.literal("__proto__").transform(() => "safe")` key produces `{}`), and it is not treated as unrecognized. In loose mode it is likewise not passed through. Any key whose *transformed* output key is `__proto__` is also dropped.
- **Discriminated unions** — an object variant with a `__proto__` field parses successfully but the output lacks an own `__proto__`.
- **`z.intersection` (v3 and v4)** — when merging object results, `__proto__` is removed from the merged object and not recursively merged. For example, `z.intersection(z.object({ name: z.string() }), z.unknown())` parsing `JSON.parse('{"__proto__":{"isAdmin":true},"name":"alice"}')` yields exactly `{ name: "alice" }`, and `z.intersection(z.record(z.string(), z.unknown()), z.any())` parsing `JSON.parse('{"__proto__":{},"a":1}')` yields `{ a: 1 }`.

No public API signatures change.
