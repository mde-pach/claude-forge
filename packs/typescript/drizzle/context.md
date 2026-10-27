Drizzle: drizzle.config.* names the schema files (`schema`), the migrations folder (`out`, default `./drizzle`) and the `dialect`.
A schema change is followed by `drizzle-kit generate`; the SQL, `meta/_journal.json` and the snapshot it writes are committed together, never written by hand.
A migration that exists in the base is not modified or deleted; a change goes in a new migration. `drizzle-kit push` needs confirmation; `push --force` is refused.
Values reach SQL through the `sql` template tag, which binds them; `sql.raw()` with a template or concatenation (or a variable built from one) is rejected.
`db.delete(t)` and `db.update(t).set(…)` are chained with `.where(…)`.
