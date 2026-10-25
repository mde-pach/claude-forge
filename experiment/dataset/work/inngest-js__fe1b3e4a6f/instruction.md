# Sandbox operation errors should be classified by replay safety and carry recovery guidance

## Problem

When a sandbox operation fails at the transport level, the `SandboxError` thrown by the sandbox client (`createSandboxClient` / `inngest.sandboxes`) is not specific enough to act on:

- Messages are generic ("Compute is temporarily unavailable" / "Sandbox operation result is ambiguous"). They do not tell callers whether to retry, check external effects, or reconcile state.
- Only Create and read operations count as safe to repeat. Destroy and file upload are reported as `operation_ambiguous`, even though repeating them is safe.
- A request can succeed (2xx) while its JSON response body is lost or unreadable mid-stream. In that case the error is a validation error or a raw error. It is not a `SandboxError` classified by the operation's replay safety.
- When the API itself returns an `operation_ambiguous` error, its generic server message is passed through unchanged.

## Requirements

### Classification

The rules apply in two cases:

- the request fails to dispatch or complete (for example, `fetch` throws);
- a successful response's body cannot be read or parsed as JSON.

This includes the file upload response.

In both cases, throw a `SandboxError` with:

- `name: "SandboxError"`;
- `action` set to the operation's action;
- the operation's `sandboxId` / `processId`, where known;
- the original error as `cause`.

Classify by action:

- **Safe to repeat**: `create`, `destroy`, `file.upload`, and all read actions (list, get, wait, file download, etc.). Use `code: "compute_unavailable"`, `ambiguous: false`, `retryable: true`.
- **Everything else**, notably `exec` (`commands.run`), `process.start`, and `process.signal`. Use `code: "operation_ambiguous"`, `ambiguous: true`, `retryable: false`.

### Messages

These are the transport/body-loss messages:

| action | message |
|---|---|
| `create` | `The Sandbox Create response was not confirmed. Repeating the same Create request is safe` |
| `destroy` | `Sandbox teardown may have been accepted. Get the sandbox; repeating Destroy is safe` |
| `file.upload` | `The sandbox file may have been replaced. Repeating the same upload is safe` |
| `exec` | `The sandbox command may have run. Inspect its external effects before running it again` |
| `process.start` | `A sandbox process may be running. List processes and reconcile before starting another` |
| `process.signal` | `The sandbox process signal may have been delivered. Get or wait for the process before sending another` |
| any other | `Compute is temporarily unavailable. Retry this operation` |

### API-returned `operation_ambiguous` errors

When the API returns an error item with code `operation_ambiguous`, replace the server's message with action-specific guidance:

- For `create`, use: `A sandbox was created, but its current resource could not be loaded. List sandboxes and reconcile by name before creating another`.
- For other actions, use the table above.

Keep the `ambiguous` / `retryable` semantics already used for API errors. For example, an ambiguous Create from the API is `ambiguous: true`, `retryable: false`.

Error items with any other code keep the server's message.

### Durable SDK errors

`action`, `ambiguous`, and `retryable` must be preserved through both the direct and durable SDK error paths.
