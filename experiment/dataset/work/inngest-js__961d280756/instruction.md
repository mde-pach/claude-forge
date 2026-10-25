# Support sandbox-level environment variables on Create

## Problem

When you create a sandbox, you can't give it any environment variables. Today, environment variables can only be set on individual commands (`commands.run`) and managed processes (process start). Even there, they *replace* the guest's default environment instead of adding to it. Users who need common configuration available to every command must repeat it on every call.

## Requirements

### New option on Create

`SandboxCreateOptions` gets a new optional field:

```ts
environment?: Record<string, string>;
```

- The field is accepted by `create` on the direct client from `createSandboxClient(...)`.
- It is also accepted by `create` on the durable/step tools from `createSandboxTools(...)` (e.g. `step.sandbox`).
- It is included in the Create request or operation input sent to the backend.
- Creating a sandbox again with the same name and the same *configuration* returns the existing sandbox. Configuration now includes `environment`, not only resources.

### Validation

Create `environment` must follow the same rules already applied to command and process `environment`:

- Keys must be nonempty and must not contain `=`. Other characters are allowed, such as dots, dashes and leading digits (e.g. `"1.WITH-DOT"`).
- Keys and values must not contain NUL (`\0`) or unpaired UTF-16 surrogates.
- There can be at most 256 entries.
- The total size must not exceed 65536 bytes. Each entry counts as the UTF-8 bytes of the key, plus 1, plus the UTF-8 bytes of the value.

Error messages must match those already produced for command and process environments.

Violations must throw or reject with `SandboxValidationError` before anything is sent:

- **Direct client:** `client.create({...})` rejects and never calls `fetch`.
- **Durable tools:** `createSandboxTools(...).create(id, {...})` rejects before the underlying tool is invoked.
- **Serialized operations:** `parseSandboxOperation({ protocolVersion: 1, action: "create", input: [ {...} ] })` throws `SandboxValidationError` for an invalid Create input. This includes an invalid `environment`, and the full Create options validation applies. In the serialized form, the timeout field is named `runningTimeoutMs`.

### Inheritance semantics

Update the documentation (JSDoc on the option types) to describe the new behaviour:

- The Create environment is inherited by all commands and managed processes in the sandbox.
- Guest default environment variables are kept.
- `environment` on a command or process now *overrides matching keys* of the sandbox environment instead of replacing it. Omitting it, or passing an empty one, inherits the sandbox environment unchanged.
- Create `environment` values are stored as literal sandbox configuration. This is **not** a secrets mechanism and must not be used for secrets.

### Release

Add a changeset describing the new feature for the `inngest` package.
