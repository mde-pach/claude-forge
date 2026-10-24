## Add an internal session-propagation toggle to the Inngest client

We are building session propagation in the SDK. Before it ships, the client needs a way to report whether the feature is enabled. Future code will check this flag. The flag must be **off by default** and must be controllable through an environment variable. There is no public client option for it yet.

### Requirements

1. **New environment variable key**
   Add an entry `InngestSessionPropagation` to the `envKeys` enum in `packages/inngest/src/helpers/consts.ts`. Its value must be `"INNGEST_SESSION_PROPAGATION"`.

2. **New internal symbol**
   `packages/inngest/src/components/Inngest.ts` must export `sessionPropagationSymbol`. Create it with `Symbol.for("inngest.sessionPropagation")`, so it is a globally registered symbol. Mark it `@internal`.

3. **Symbol-keyed getter on the client**
   An `Inngest` instance must expose a read-only boolean property keyed by this symbol, used as `inngest[sessionPropagationSymbol]`:
   - If `INNGEST_SESSION_PROPAGATION` is unset, or cannot be parsed as a boolean, it returns `false`.
   - It returns `true` when the variable is set to a truthy boolean string, such as `"true"` or `"1"`.
   - It returns `false` when the variable is set to a falsy boolean string, such as `"false"` or `"0"`.
   - Parse the value the same way the SDK already parses other boolean env vars, such as `INNGEST_DEV`.

4. **Lazy resolution**
   Compute the value on every access from the client's current env, the same way `mode` is resolved. Do not cache it at construction time. This matters because env vars can be supplied after the client is constructed, for example via `setEnvVars` in edge or serverless runtimes like Cloudflare Workers, and those values must be honoured.

5. **No public option**
   Do not add a `ClientOptions` field for this flag. The env var is the only way to configure it.
