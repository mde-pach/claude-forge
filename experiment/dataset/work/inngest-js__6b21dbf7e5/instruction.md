# Make sandbox Create safe to retry (reuse active sandboxes by name)

## Background

Active sandbox names are now idempotent on the server. If a client creates a sandbox with the same `name` (and the same resources) as an existing active sandbox, the API returns that existing sandbox with HTTP **200**. It does not create a new one.

The SDK does not yet take advantage of this. As a result, Create is treated as an unsafe mutation, and failed Create calls are not retried.

## Current problems

1. **Existing sandboxes are rejected.** `inngest.sandboxes.create(options)` (the client returned by `createSandboxClient(...)`) accepts only `201`/`202` responses from `POST /v2/sandboxes`. A `200` response carrying the existing sandbox is treated as an error.
2. **Transport failures are marked ambiguous.** When the underlying `fetch` throws during Create (for example, `TypeError("connection refused")`), the SDK raises a `SandboxError` with:
   - `code: "operation_ambiguous"`
   - `ambiguous: true`
   - `retryable: false`

   Inside a durable function, this makes `step.sandbox.create(...)` fail permanently instead of retrying.

## Required behaviour

### Accepting existing sandboxes

`create` must accept a `200` response, in addition to `201` and `202`. It must return the sandbox reference from the response body unchanged.

- This applies whatever the sandbox's status is: `PENDING`, `STARTING`, `RUNNING`, `PAUSED`, `TERMINATING`, `TERMINATED` or `FAILED`.
- The returned `id` and `status` must match the response.
- Only a single request is made.

### Transport failures during Create

A transport-level failure (the fetch throws) during Create must be classified the same way as a failure during a safe read action such as `list`. The rejected error is a `SandboxError` with:

- `name: "SandboxError"`
- `action: "create"`
- `code: "compute_unavailable"`
- message `"Compute is temporarily unavailable"`
- `ambiguous: false`
- `retryable: true`

Transport failures for other mutating actions keep their current behaviour (`operation_ambiguous`, ambiguous, not retryable).

### Behaviour inside durable functions

With `sandboxMiddleware()`, when `step.sandbox.create(id, options)` hits either of the following failures:

- a transport failure, or
- a structured API error response such as HTTP 503 with `{ errors: [{ code: "compute_unavailable", message: "Compute is temporarily unavailable" }] }`

then:

- The step must be reported as a retriable step error (`StepError`), not a non-retriable `StepFailed`.
- When replayed with that stored error, the function must see a reconstructed `SandboxError` with the following fields preserved:
  - `name`, `action: "create"`
  - `code: "compute_unavailable"`
  - `ambiguous: false`
  - `retryable: true`
- This must match the existing behaviour for `step.sandbox.list(...)`.

### Documentation

Document on `SandboxCreateOptions.name` that it is the stable identity for an active sandbox. Creating the same name again with the same resources returns the existing sandbox.
