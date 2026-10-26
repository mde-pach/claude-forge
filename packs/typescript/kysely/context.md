Kysely: values reach SQL through the sql`...` tag, which binds them, and names through `sql.ref()` / `sql.id()` / `sql.table()`; `sql.raw()` and `sql.lit()` built from a template or concatenation are rejected (unescaped).
`deleteFrom()` and `updateTable()` chains have a `where…()` before `execute()`.
