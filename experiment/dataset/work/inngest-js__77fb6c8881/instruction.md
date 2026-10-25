# Propagate run sessions onto events sent with `step.sendEvent`

## Problem

Events can carry session metadata (`meta.sessions`) that groups runs together. When a function run sends further events with `step.sendEvent`, those events do not inherit the parent run's sessions. The child runs therefore fall out of the parent's session groupings. There is also no client option for turning session propagation on or off; it can only be set through the `INNGEST_SESSION_PROPAGATION` env var.

## Requirements

### Client option
- Add `sessionPropagation?: boolean` to `ClientOptions`. This option is experimental.
- The internal toggle (`client[sessionPropagationSymbol]`) resolves in this order:
  1. An explicit boolean option wins outright, in either direction, even when the env var conflicts.
  2. Otherwise the `INNGEST_SESSION_PROPAGATION` env var applies.
  3. Otherwise the existing default (off) applies.

### Run context
- Add an optional `sessions?: Record<string, string>` to the handler context (`BaseContext`).
- When propagation is enabled, populate `ctx.sessions` from the run's triggering events. Leave it unset when the result is empty.
- This step is best-effort. Any failure must leave `ctx.sessions` absent and must never fail the run.
- When propagation is disabled, `ctx.sessions` stays unset.

### Session reduction
Export `reduceEventsToPropagatedSessions(events)` from `packages/inngest/src/helpers/sessions.ts`.
- **Input:** an array of `{ meta?: EventMeta | null }`.
- **Output:** a `Record<string, string>`.
- **Behaviour:**
  - Union the `meta.sessions` of all events, skipping empty keys.
  - Canonicalise ids with `String()`, so `1` and `"1"` are the same id.
  - Drop any key that has more than one distinct id across the events.
  - Drop conflicting keys *before* truncating.
  - Sort the remaining keys by UTF-8 byte order and keep the first 5.
  - Keys such as `__proto__` must end up as own properties of the result.
  - Empty input, or input with no sessions, returns `{}`.

Export `compareUtf8(a, b): number` from the same module. It is a comparator over UTF-8 byte sequences:
- It returns 0 for equal strings, a negative number when `a` sorts first, and a positive number otherwise.
- A prefix sorts before its extension.
- The result must follow UTF-8 byte order, not JavaScript's UTF-16 code-unit order. For example, `"\uFFFF"` sorts before `"\u{1F600}"`.

### Stamping outgoing events
- Add `propagated_sessions?: EventSessions` to `EventMeta`. It is experimental and not meant to be set by hand.
- Event meta normalisation must preserve and normalise `propagated_sessions` alongside `sessions`.
- Export `stampPropagatedSessions(payload, sessions)` from `packages/inngest/src/components/InngestStepTools.ts`. It takes a `SendEventPayload` and `Record<string,string> | undefined`.
  - If `sessions` is undefined or empty, return the *same* payload object.
  - Otherwise return a new payload, or a new array of payloads, in which each event's `meta.propagated_sessions` is a copy of `sessions`.
  - Keep any existing `meta` fields, including manual `meta.sessions`, untouched.
  - Never mutate the input.
- When the toggle is on, `step.sendEvent` must stamp its payload with `ctx.sessions`, read at send time, before sending.
- When the toggle is off, the payload is sent exactly as given.
