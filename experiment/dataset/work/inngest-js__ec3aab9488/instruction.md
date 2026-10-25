# Support `null` tombstones in event sessions to override propagated sessions

## Problem

When an event is sent, its `meta` can carry two session layers:

- `sessions`: the manual layer, which the user writes.
- `propagated_sessions`: the inherited layer, stamped automatically from `ctx.sessions`.

Users currently have no way to override or clear an inherited session. A `null` session value is rejected with `Event session "<key>" must be a string or number`.

## Required behaviour

The manual `meta.sessions` layer should behave as an RFC 7386 (JSON Merge Patch) against the propagated layer:

- **Per-key tombstone.** A `null` value for a key means "cut the inherited session of this key". It is accepted and kept as `null` in the payload sent over the wire. For example, `meta: { sessions: { conversation_id: null, keep: "1" } }` is sent as `{ conversation_id: null, keep: "1" }`.
- **Clear all.** Setting the whole field to `null` (`meta: { sessions: null }`) means "clear all inherited sessions". It is kept as `sessions: null` in the sent meta, and any `propagated_sessions` are still sent alongside it.
- **Empty or absent input.** An empty object, or an absent field, normalizes to "absent", the same as today.
- **Other value types.** Values that are not a string, number or `null` are still rejected on the manual layer. The error message becomes `Event session "<key>" must be a string, number, or null`.

The propagated layer `meta.propagated_sessions` does not accept tombstones:

- A per-key `null` throws `Event session "<key>" must be a string or number`.
- A whole-field `null` is dropped and treated as absent.

All existing validation (empty keys, finite numbers, number-to-string normalization, etc.) stays the same on both layers.

When reducing a run's triggering events into propagated sessions, any `null` session values must be skipped.

## Interfaces

### Types

- `EventSessionValue` becomes `string | number | null`.
- `EventMeta.sessions` is typed `EventSessions | null`.
- Add the following new types and export them publicly from the package root (`inngest`):
  - `PropagatedEventSessionValue`: `EventSessionValue` without `null`.
  - `PropagatedEventSessions`: `Record<string, PropagatedEventSessionValue>`.
- `EventMeta.propagated_sessions` is typed `PropagatedEventSessions`.

### Helpers in `packages/inngest/src/helpers/sessions.ts`

`normalizeEventSessions` is replaced by two exported functions:

- `normalizeManualSessions(sessions)`
  - Returns `Record<string, string | null>` when there are sessions.
  - Returns `null` for a whole-field `null`.
  - Returns `undefined` when the input is absent or empty.
- `normalizePropagatedSessions(sessions)`
  - Returns `Record<string, string>`, or `undefined`.

`normalizeEventMeta(meta)` uses the manual normalizer for `sessions` and the propagated normalizer for `propagated_sessions`. Its behaviour:

- It returns `undefined` when neither layer is set, including for `{}` and `undefined`.
- Otherwise it returns an object containing whichever layers are present.
- `sessions: null` is kept in the output and is not treated as absent.
