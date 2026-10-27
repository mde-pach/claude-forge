Temporal: workflow code runs in a deterministic sandbox. A file importing `@temporalio/workflow` imports no Node built-ins and no `@temporalio/activity` / `client` / `worker`; activities are reached through `proxyActivities<typeof activities>()` with `import type * as activities`.
In workflows, `uuid4()` from `@temporalio/workflow` replaces `crypto.randomUUID()`.
