# Custom `user.modelName` that matches another table's schema key resolves to the wrong table

## Problem

If `user.modelName` is set to a string that is also the schema key of a different built-in table, the user table's alias is lost. For example:

```ts
{ user: { modelName: "account" }, account: { modelName: "identity" } }
```

Two things go wrong with this configuration:

1. **Foreign key references point at the wrong table.** The schema from `getAuthTables(options)` (in `packages/core`) builds the `userId` foreign keys on `session` and `account` with `references.model` set to the user's custom `modelName`. Consumers treat `references.model` as a schema key and look it up as `tables[references.model]`. With the configuration above, they get the OAuth account table, not the user table.

2. **Adapter model resolution picks the wrong table.** The resolver from `initGetDefaultModelName({ schema, usePlural })` maps a model string (a schema key or a custom `modelName`) to its canonical schema key. It checks schema keys before `modelName` aliases, so `"account"` resolves to `"account"` even though the user explicitly assigned `"account"` as the user table's name. As a result, joins and queries hit the wrong table.

## Required behaviour

### `getAuthTables(options)`

- `session.fields.userId.references.model` must always be the canonical user schema key, `"user"`, regardless of `user.modelName`.
- `account.fields.userId.references.model` must always be the canonical user schema key, `"user"`, regardless of `user.modelName`.
- With the collision configuration above, `tables[ref.model]` for both references must be the user table. That table has `modelName === "account"` and has an `email` field.

### `initGetDefaultModelName({ schema, usePlural })` → `(model: string) => string`

When resolving a candidate string:

- A table whose `modelName` equals the candidate takes precedence over a table whose schema key equals the candidate.
- If no `modelName` matches, fall back to the schema key match.
- When `usePlural` is true and the input ends in `s`, first try resolving the input with the trailing `s` removed, using the same precedence. If that fails, try the full input.
- If nothing resolves, throw `BetterAuthError` with the message `Model "<model>" not found in schema`.

Expected results:

- **Default schema:** `"user"`, `"account"` and `"session"` return themselves.
- **Default schema with `usePlural: true`:** `"users"` returns `"user"` and `"accounts"` returns `"account"`.
- **`user.modelName: "users_table"`:** both `"users_table"` and `"user"` return `"user"`.
- **Collision configuration above:**
  - `"account"` returns `"user"`.
  - `"identity"` returns `"account"`.
  - `"user"`, `"session"` and `"verification"` return themselves.
- **Unknown input:** `"does_not_exist"` throws `Model "does_not_exist" not found in schema`.
