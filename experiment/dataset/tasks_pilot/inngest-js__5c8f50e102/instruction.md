You are working in the git repository in the current directory. Make the change described below. Work autonomously: nobody will answer questions.

# Support `Temporal.*` values in timeout and sleep fields

## Problem

Timeout fields in the SDK accept only a `number` of milliseconds, an `ms`-compatible string, or a `Date`. Users who work with the Temporal API cannot pass `Temporal.Duration`, `Temporal.Instant` or `Temporal.ZonedDateTime` values. `step.sleep` accepts a `Temporal.Duration`, but it converts it without a reference date. As a result, calendar units such as months or years fail or give the wrong result.

## Requirements

### Accepted timeout types

These fields must accept, in both their types and at runtime, `number | string | Date | Temporal.DurationLike | Temporal.InstantLike | Temporal.ZonedDateTimeLike`:

- `step.waitForEvent(id, { timeout })`
- `step.waitForSignal(id, { timeout })`
- `step.invoke(id, { timeout })`. Runtime validation must accept the Temporal values and still reject other values.
- `cancelOn[].timeout` (the `Cancellation` type)

### Serialisation (`timeStr` in `src/helpers/strings.ts`)

`timeStr(input)` must accept every type listed above:

- **`Date`**: unchanged. It returns `toISOString()`.
- **`Temporal.Instant` or `Temporal.ZonedDateTime`**: returns the absolute instant as an ISO string, equal to `instant.toString()` or `zdt.toInstant().toString()`. For example, `Temporal.Instant.from("1970-01-01T00:00:00Z")` becomes `"1970-01-01T00:00:00Z"`.
- **`Temporal.Duration`**: converted to total milliseconds, then formatted the same way as numbers and strings. For example, `{ minutes: 1 }` becomes `"1m"` and `{ hours: 2 }` becomes `"2h"`.
  - Durations must produce the same output as the equivalent `ms` strings. For example, `{ weeks: 1 }` matches `"1w"`, and `{ hours: 1, minutes: 30 }` matches `"1.5 hours"`.
  - Calendar units (months, years, weeks) are resolved relative to the current time in UTC.
  - Example with the current time fixed at `2026-04-28T12:00:00Z`: `{ months: 1 }` becomes `"4w2d"` and `{ years: 1 }` becomes `"52w1d"`.
  - The conversion must work whichever Temporal implementation created the Duration.

### Resulting op output

- `step.waitForEvent` and `step.invoke` must put the serialised value in `opts.timeout`. For example, a 2-hour Duration gives `"2h`" and an Instant gives `instant.toString()`.
- `step.sleep` with a Duration must return the serialised string as the op `name`. For example, with the time fixed as above, `step.sleep("id", Temporal.Duration.from({ months: 1 }))` returns `name: "4w2d"`.

### Middleware sleep validation

- Rename the helper `isTimeStrInput` in `src/components/middleware/utils.ts` to `isSleepInput(value: unknown)`.
- It must return `true` for a string, number, `Date` or `Temporal.Duration`, and `false` for anything else, such as `{}`.
- When middleware gets an invalid sleep input, the error message must be exactly: `Sleep time must be a string, number, Date, or Temporal.Duration`.

### Docs

Update the doc comments on these fields to list the new accepted types.
