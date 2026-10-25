You are working in the git repository in the current directory. Make the change described below. Work autonomously: nobody will answer questions.

## `step.ai.wrap` is reported to middleware as an `"unknown"` step type

### Problem

When a function uses `step.ai.wrap(...)`, the SDK logs an "Unknown step type" warning. Middleware also receives `stepType: "unknown"` for that step instead of `"ai.wrap"`.

`step.ai.wrap` runs its wrapped callback like a regular `step.run`. Its planned or executed op is therefore emitted with the regular step op codes (for example `StepOpCode.StepPlanned`), with `opts.type` set to `"step.ai.wrap"`. It is not emitted with `StepOpCode.AiGateway`. The current op-code-to-step-type mapping only recognises `"step.ai.wrap"` under `AiGateway`, so the real op is never matched and falls through to `"unknown"`.

### Requirements

The step-type mapping helper must classify `step.ai.wrap` ops correctly. The helper is `stepTypeFromOpCode`, exported from `packages/inngest/src/components/middleware/utils.ts`, with this call shape:

```ts
stepTypeFromOpCode(op: StepOpCode, opts: { type?: string } | undefined, logger): StepType
```

Required behaviour:

- **`step.ai.wrap` ops:** a `StepOpCode.StepPlanned` op (and the other run-style step op codes handled alongside `step.run`) with `opts.type === "step.ai.wrap"` must return `"ai.wrap"`. It must not log an unknown-step-type warning.
- **`AiGateway` ops:** a `StepOpCode.AiGateway` op with `opts.type === "step.ai.wrap"` is no longer a recognised combination. It must be treated like any other unrecognised `AiGateway` type and return `"unknown"`.
- **Unchanged mappings:** these must keep their current results:
  - `AiGateway` with `"step.ai.infer"` returns `"ai.infer"`.
  - `StepPlanned` with no `type` returns `"run"`.
  - `StepPlanned` with `"step.sendEvent"` returns `"sendEvent"`.
  - `Gateway` returns `"fetch"`.
  - All other existing mappings, and the existing fallbacks to `"unknown"`, stay as they are.
- **End-to-end:** middleware hooks that receive step info (e.g. `stepType`) must see `"ai.wrap"` for steps created with `step.ai.wrap`.
