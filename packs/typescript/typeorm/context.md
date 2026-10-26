TypeORM: schema changes go through migrations (`migration:generate`, `migration:run`); `synchronize: true` and `dropSchema: true` are rejected, `schema:sync` / `schema:drop` are refused and `migration:revert` needs confirmation.
Raw `.query()` takes its values as parameters (`query(sql, [values])`); query builder conditions and orderings use `:name` parameters, never `${}` or concatenation.
