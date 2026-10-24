## Unhelpful error message for non-finite numbers in `z.number()` (v4)

In Zod v4, `z.number()` rejects non-finite values by default. Parsing `Infinity` or `-Infinity` fails with an `invalid_type` issue, but the English error message is:

```
Invalid input: expected number, received number
```

This names the same type on both sides and does not tell the user what was wrong.

The issue data is also inaccurate. The issue's `received` field is always the literal `"Infinity"`, so for `-Infinity` the sign is lost.

### Expected behaviour

For a `z.number()` schema, including one with `.finite()`, parsing a non-finite number must produce a single `invalid_type` issue with `expected: "number"`, `path: []`, and the values below.

| Input | `received` | `message` (English locale) |
|---|---|---|
| `Infinity` | `"Infinity"` | `Invalid input: expected number, received Infinity` |
| `-Infinity` | `"-Infinity"` | `Invalid input: expected number, received -Infinity` |
| `NaN` | `"NaN"` (unchanged) | `Invalid input: expected number, received NaN` (unchanged) |

Requirements:

- **`received` field:** it must contain the string form of the actual non-finite input, preserving the sign. This applies regardless of which locale is active.
- **English message:** when the received input is a non-finite number, the message must show that value instead of the generic type name `number`.
- **Other `invalid_type` messages:** wording for all other inputs and types must stay the same. For example, a string passed to `z.number()` still reads `expected number, received string`.
- **Other locales:** only the English wording needs to change. Non-English messages may keep their current wording.
- **`util.parsedType`:** this function is exported publicly via `util`. Its behaviour must not change, so it still returns `"number"` for `Infinity`.
