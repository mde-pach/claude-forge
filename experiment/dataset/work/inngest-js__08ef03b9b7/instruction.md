# Parallel steps can hang when `optimizeParallelism` is `false`

## Problem

A function can opt out of optimized parallelism by setting `optimizeParallelism: false`, either on the function or on the client. When it does, its runs should execute with execution version **V1** (`ExecutionVersion.V1`). The default in v4 is V2 (`ExecutionVersion.V2`), which coalesces parallel steps.

Today the opt-out is only honoured on the first request of a run, when the incoming request body has `version: -1` ("SDK decides"). Later requests in the same run carry an explicit version from the executor, usually `2`. On those requests the SDK ignores the opt-out and executes and responds as V2.

This lets the executor switch the run back to V2 and start coalescing parallel steps. It can then race and hang the run.

## Required behaviour

When the communication handler executes a function, it picks the execution version and reports it to the executor. The SDK reports this version in the request-version response header (`x-inngest-req-version`).

- If `optimizeParallelism` is `false`, at either the function or the client level, the function must always run with `ExecutionVersion.V1`. This applies whatever version the incoming request body specifies, including:
  - `-1` (SDK decides), and
  - an explicit `2`.

  The response header must report `1` in every one of these cases.
- If optimized parallelism is not disabled, behaviour is unchanged. With `version: -1`, the handler uses `ExecutionVersion.V2` and reports `2`.

## Example

Consider a function created with `{ id: "fn", optimizeParallelism: false, triggers: [...] }`. It receives a request whose body contains `version: 2`. The handler must execute it as V1, and the response's request-version header must be `"1"`.
