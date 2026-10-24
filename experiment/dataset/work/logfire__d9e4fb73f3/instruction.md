# Bound the length of INSERT statement summaries

## Problem

`logfire._internal.db_statement_summary.message_from_db_statement` turns a DB statement into a short span message. SELECT statements are kept within the maximum query-message length, but INSERT statements are not. The columns and values parts of an INSERT summary are already truncated to 25 characters each. The table name is never truncated, so a long, schema-qualified, quoted table name makes the message arbitrarily long.

## Required behaviour

INSERT summaries keep their current shape:

```
INSERT INTO <table> <columns> VALUES <values>
```

- **Columns and values:** truncation is unchanged. Each is still truncated to 25 characters with the existing middle-ellipsis (`…`) truncation.
- **Table name:** it gets whatever length budget is left after the other parts. The budget is the module's maximum query-message length (the same limit the SELECT path uses) minus the length of the `INSERT INTO ` prefix and the rendered ` <columns> VALUES <values>` suffix.
  - Apply the same middle-ellipsis truncation helper used elsewhere in the module, so the start and end of the name are kept around a `…`.
  - The table is the best identifier of the query, so it is the part that gives up length. Shorter columns and values leave more room for the table name.
- **Short tables:** if the table name already fits, the output is the same as today.

## Examples

These use `message_from_db_statement(attrs, None, 'INSERT')`, where `attrs` has `db.statement` set to the query.

**Long table name with long columns.** The query:

```
INSERT INTO "analytics"."very_long_events_table_name_for_testing" (apple, banana, carrot, durian, egg, fig) VALUES (1, 2, 3, 4, 5, 6)
```

must produce exactly:

```
INSERT INTO "analyti…testing" (apple, bana…n, egg, fig) VALUES (1, 2, 3, 4, 5, 6)
```

**Long table name with short columns and values.** The query:

```
INSERT INTO "analytics"."very_long_events_table_name_for_testing" (apple) VALUES (12345678901234567890)
```

must produce exactly:

```
INSERT INTO "analytics"."ve…me_for_testing" (apple) VALUES (12345678901234567890)
```

**Short table name.** Output is unchanged. For example, the query:

```
INSERT INTO table (apple, banana, carrot, durian, egg, fig) VALUES (1, 2, 3, 4, 5, 6)
```

still produces:

```
INSERT INTO table (apple, bana…n, egg, fig) VALUES (1, 2, 3, 4, 5, 6)
```
