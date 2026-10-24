# Middleware does not recognise `waitForSignal` steps

## Problem

Middleware receives a step type for each step a function runs, as `Middleware.StepType` (exported from the `Middleware` namespace in the middleware component). This union covers step kinds such as `"sendEvent"`, `"sleep"` and `"waitForEvent"`, but it has no member for signal waits. A step created with `step.waitForSignal(...)` uses the `StepOpCode.WaitForSignal` op code, and that op code is not mapped to any step type.

As a result, middleware cannot identify a signal-wait step. The SDK logs an "Unknown step type" warning and falls back to the default instead of reporting a dedicated type. Middleware authors also cannot narrow on a `"waitForSignal"` type in TypeScript, because it is not part of the union.

## Requirements

- Add the string literal `"waitForSignal"` as a member of the `Middleware.StepType` union, alongside the existing members.
- The helper `stepTypeFromOpCode(op, opts, logger)` in `packages/inngest/src/components/middleware/utils.ts` must return `"waitForSignal"` when called with `StepOpCode.WaitForSignal`, for example:

  ```ts
  stepTypeFromOpCode(StepOpCode.WaitForSignal, undefined, logger); // "waitForSignal"
  ```

  This mapping must not depend on `opts`, and it must not log the "Unknown step type" warning.
- The step info passed to middleware hooks for a `step.waitForSignal` step must report the step type `"waitForSignal"`.
- Mappings for all other op codes must not change.
