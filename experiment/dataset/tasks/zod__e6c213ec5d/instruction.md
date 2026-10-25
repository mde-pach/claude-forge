You are working in the git repository in the current directory. Make the change described below. Work autonomously: nobody will answer questions.

# JSON Schema conversion silently drops `__proto__` keys

## Problem

Zod's JSON Schema converters (`z.toJSONSchema` and `fromJSONSchema`) lose any entry whose key is `__proto__`. Assigning to a plain object with that key invokes the inherited prototype setter instead of creating an own property, so the entry disappears without error. Normal parsing already supports a `__proto__` key in an object shape; only the converters do not.

The results are incorrect or self-contradictory:

- **`fromJSONSchema`**: when a JSON Schema object declares a `__proto__` property (and lists it in `required`), the generated Zod object has no such field. A schema that should require `__proto__: "admin"` accepts input that lacks it or that has the wrong value.
- **`fromJSONSchema` (annotations)**: an unrecognized `__proto__` keyword on a schema, such as `{ "type": "string", "__proto__": { "custom": 1 } }`, never reaches the metadata stored in the supplied `registry`.
- **`z.toJSONSchema(schema)`**: for `z.object({ ["__proto__"]: z.literal("admin"), role: z.string() })`, the output lists `"__proto__"` in `required` but leaves it out of `properties`.
- **`z.toJSONSchema(registry)`**: a schema registered under the id `"__proto__"` is missing from the returned `schemas` map.
- **`$defs`**: when a schema with id `"__proto__"` is extracted into `$defs` (for example via the `metadata` option), the output still contains `$ref: "#/$defs/__proto__"`, but `$defs` has no matching entry. The reference cannot be resolved.

JSON Schemas usually come from `JSON.parse`, which does create own `__proto__` keys. That makes this a realistic case.

## Requirements

Treat `__proto__` like any other key everywhere in both converters, as an **own, enumerable property**:

1. **`fromJSONSchema`**
   - A `__proto__` entry in `properties` becomes a field in the resulting object shape, and its required/optional status follows `required`.
   - Parsing then enforces that field. For a required `__proto__` with `const: "admin"`:
     - `{ role: "x" }` fails.
     - A JSON-parsed `{ "role": "x", "__proto__": "wrong" }` fails.
     - A JSON-parsed `{ "role": "x", "__proto__": "admin" }` succeeds.
   - An unrecognized `__proto__` keyword is stored as an own property of the metadata object in the provided registry, with its original value.

2. **`z.toJSONSchema` with a schema**
   - `properties` contains an own `__proto__` entry. Every key in `required` must have a matching own entry in `properties`.
   - After a `JSON.stringify` round-trip, the entry is preserved. For `z.literal("admin")` it is `{ type: "string", const: "admin" }`, with `io: "input"`.

3. **`z.toJSONSchema` with a registry**
   - The `schemas` result has an own `"__proto__"` key. Key order follows registration order, e.g. `["__proto__", "normal"]`.

4. **`$defs`**
   - A definition with id `"__proto__"` appears as an own entry in `$defs`, so `"#/$defs/__proto__"` references resolve, including after a JSON round-trip.

Output for all other keys must stay the same.
