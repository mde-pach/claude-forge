Untrusted input is validated with Zod at boundaries. Types derive from schemas with `z.infer<typeof Schema>`.
Zod 4: string formats are top-level (`z.email()`, `z.uuid()`, `z.url()`, `z.iso.datetime()`), not `z.string().email()`.
Custom messages use the single `error` parameter (not `message`, `required_error`, `invalid_type_error` or `errorMap`).
Object strictness: `z.strictObject()` and `z.looseObject()`. Errors: `error.issues`, `z.treeifyError()`, `z.flattenError()`.
