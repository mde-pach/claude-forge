## Sandbox: optionally wait for a sandbox to reach `RUNNING`

Creating a sandbox today returns as soon as Create is accepted. The returned sandbox may still be `STARTING`, and there is no supported way to wait for it to become usable. Add an opt-in way to wait for readiness. The current Create behaviour must stay the default.

### Requirements

**Create option**
- `SandboxCreateOptions` gains an optional `runningTimeout?: SandboxDuration`. It uses the same duration format as other sandbox timeouts, e.g. `"5s"`, and its maximum is 5 minutes.
- When `runningTimeout` is omitted, Create behaves exactly as before. It sends one POST and returns immediately with `STARTING` or `RUNNING`.
- When `runningTimeout` is supplied:
  - It is not sent in the Create request body; the body stays `{ name, vcpu, memoryMb }`.
  - The client waits until the sandbox reaches `RUNNING` and then returns it.
  - Create is never re-sent while waiting. Readiness is checked by GET-ing the sandbox, about once per second.

**`waitUntilRunning`**
- Direct client: `sandbox.waitUntilRunning({ timeout }): Promise<Sandbox>`.
- Durable step surface: `sandbox.waitUntilRunning(idOrOptions, { timeout }): Promise<DurableSandbox>`. It runs as its own step and is memoized and rehydrated on replay like other sandbox steps.
- Export a new type `SandboxWaitUntilRunningOptions` (`{ timeout: SandboxDuration }`) from the same public entry points as the other sandbox types, including `inngest/experimental`.
- `timeout` is required and has a 5-minute maximum. Unknown keys are rejected with a validation error.

**Waiting semantics** (shared by Create-with-timeout and `waitUntilRunning`)
- If the sandbox is already `RUNNING`, return it without any request.
- `PENDING` and `STARTING` count as still starting; keep polling.
- If a read fails with a retryable `SandboxError` (e.g. a network failure), keep polling within the deadline. Non-retryable errors are rethrown.
- Any other status throws a `SandboxError` with:
  - `code: "sandbox_start_failed"`, `retryable: false`, and `sandboxId`;
  - `action` set to `"create"` or `"waitUntilRunning"`, depending on the caller;
  - message `` `Sandbox entered ${status} before reaching RUNNING` ``;
  - `details: [{ status, error? }]`, where `error` is the sandbox's error when present.
- If the deadline passes first, throw a `SandboxError` with:
  - `code: "sandbox_start_timed_out"`, `retryable: false`, `sandboxId`, and the same `action` rule;
  - message `` `Sandbox did not reach RUNNING within ${timeoutMs} milliseconds` ``;
  - `details: [{ status, timeoutMs }]`.
- Add `"sandbox_start_failed"` and `"sandbox_start_timed_out"` to `SandboxErrorCode`, and `"waitUntilRunning"` to `SandboxAction`.

**Durable wire protocol**
- The durable `create` operation input may carry an optional `runningTimeoutMs`, a positive integer of at most 300000. For example, `runningTimeout: "5s"` serializes as `runningTimeoutMs: 5000`.
- Add a new operation `action: "waitUntilRunning"` with:
  - `target: { sandbox }`;
  - `input: [{ timeoutMs }]`, where `"5s"` becomes `5000`.
- Its result is `{ protocolVersion: 1, action: "waitUntilRunning", sandbox }`. The result is only valid if the sandbox status is `RUNNING`.
- `waitUntilRunning` is a safe, read-only action, like `get`.
