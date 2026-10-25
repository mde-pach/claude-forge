You are working in the git repository in the current directory. Make the change described below. Work autonomously: nobody will answer questions.

## Drizzle schema generator emits duplicate unique constraints for fields that are both `unique` and `index`

### Problem

The CLI's Drizzle schema generator (`generateDrizzleSchema`, exported from the CLI package's generators) can produce two unique constraints for one column. This happens when a field in the Better Auth schema (core or plugin-provided) has both `unique: true` and `index: true`.

For such a field, the generated code currently contains two things:

- the column definition with `.unique()` chained on it, for example `slug: text("slug").notNull().unique()`;
- a separate `uniqueIndex("<model>_<field>_uidx")` entry in the table's index callback, with `uniqueIndex` added to the `drizzle-orm/<dialect>-core` import.

The result is two unique constraints or indexes on the same column. Migrations generated from this schema are redundant and can conflict.

### Expected behaviour

When a field is declared with both `unique: true` and `index: true`:

- The column definition still carries the unique constraint, for example `slug: text("slug").notNull().unique()` for a required string field named `slug` on the `pg` provider.
- No `uniqueIndex(...)` entry is emitted for that field.
- No index name ending in `_uidx` is emitted, such as `testTable_slug_uidx`.
- `uniqueIndex` is never added to the dialect-core import list, because the generator no longer emits it at all.

Behaviour for other cases is unchanged:

- Fields with `index: true` and no `unique` still get a regular `index("<model>_<field>_idx")` entry, and `index` is still imported when needed.
- Fields with only `unique: true` still get `.unique()` on the column.

### Example

A plugin schema like this:

```ts
schema: {
  testTable: {
    fields: {
      slug: { type: "string", index: true, unique: true },
    },
  },
}
```

With the `pg` provider, the output must contain `slug: text("slug").notNull().unique()`. The string `uniqueIndex` and the string `slug_uidx` must not appear anywhere in the output.
