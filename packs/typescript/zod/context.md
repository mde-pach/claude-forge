Zod validates untrusted input at boundaries; types derive from schemas with `z.infer<typeof Schema>`.
Zod 4: string formats are top-level (`z.email()`, `z.uuid()`, `z.url()`, `z.iso.datetime()`); `z.string().email()` and the like are rejected.
Custom messages use the single `error` parameter (`message`, `required_error`, `invalid_type_error` and `errorMap` are rejected).
Object strictness: `z.strictObject()` and `z.looseObject()`. Errors: `error.issues`, `z.treeifyError()`, `z.flattenError()`.
