## Range issues on date schemas report the wrong `origin` when the bound is a number

### Problem

In Zod v4, a date schema can be bounded with either a `Date` or a numeric timestamp in milliseconds. When the bound is a number, a failing check reports the wrong origin:

```ts
z.date().min(someDate.getTime()).safeParse(earlierDate);
// issue.origin === "number"   ← wrong, should be "date"
```

The issue's `origin` comes from the type of the bound, not from the value being validated. This also changes the default error message: it reads "expected number to be …" instead of "expected date to be …".

The same thing happens in `zod/v4-mini` with `z.date().check(z.gte(<number>))` and `z.date().check(z.lte(<number>))`. Mini has no `.min()`/`.max()` methods, so the fix has to work at the level of the check itself.

### Required behaviour

For the greater-than/greater-than-or-equal checks (`too_small`) and the less-than/less-than-or-equal checks (`too_big`), the issue's `origin` must come from the runtime type of the value being validated:

- a `Date` value gives `origin: "date"`
- a `number` value gives `origin: "number"`
- a `bigint` value gives `origin: "bigint"`

The type of the bound must not affect the origin. The existing multiple-of, length and size checks already work this way.

This applies to:
- `zod` / `zod/v4`: `z.date().min(n)`, `z.date().max(n)`, and the equivalent `.gt`/`.gte`/`.lt`/`.lte` style checks.
- `zod/v4-mini`: `z.date().check(z.gte(n))`, `z.date().check(z.lte(n))`, `z.gt`, `z.lt`.

Everything else about the issue stays the same:
- `minimum` / `maximum` is still reported as the numeric timestamp. A `Date` bound is still converted with `getTime()`.
- `inclusive` and `input` are unchanged.

### Example

With a bound of `1667606400000` (2022-11-05 UTC), parsing an earlier date with `z.date().min(1667606400000)` must produce:

```
{
  code: "too_small",
  origin: "date",
  minimum: 1667606400000,
  inclusive: true,
  message: "Too small: expected date to be >=1667606400000",
  path: []
}
```

Parsing a later date with `z.date().max(1667606400000)` must produce:

```
{
  code: "too_big",
  origin: "date",
  maximum: 1667606400000,
  inclusive: true,
  message: "Too big: expected date to be <=1667606400000",
  path: []
}
```

Number and bigint schemas must keep reporting `"number"` and `"bigint"` as before.
