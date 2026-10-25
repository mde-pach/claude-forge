You are working in the git repository in the current directory. Make the change described below. Work autonomously: nobody will answer questions.

## Last-login-method plugin: allow vetoing the cookie (e.g. for GDPR consent)

The `lastLoginMethod` plugin (from `better-auth/plugins`) always writes the last-used login method cookie after a successful sign-in or sign-up. Some applications are not allowed to store this cookie until the user has consented, for example under GDPR or similar regulations. Right now they have no way to prevent it short of disabling the plugin.

### Requirement

Add a new optional option to `LastLoginMethodOptions`:

```ts
beforeStoreCookie?: (
  ctx: GenericEndpointContext,
  lastUsedLoginMethod: string,
) => Promise<boolean> | boolean
```

**Arguments**

- The hook is called each time the plugin is about to set the last-login-method cookie.
- `ctx` is the endpoint context of the request that triggered the login. For example, `ctx.path` is `"/sign-in/email"` for an email sign-in.
- `lastUsedLoginMethod` is the method that was detected, such as `"email"` or `"google"`.

**Return value**

- If the hook returns a truthy value, or a promise that resolves to one, the cookie is set as it is today.
- If it returns a falsy value, or a promise that resolves to one, the cookie is not set. Falsy values include `false` and `0`.
- The hook is evaluated on every login. A decision made on one login does not affect later ones, so consent can change between logins.

**Errors**

- If the hook throws synchronously or returns a rejected promise, the cookie is not set.
- The error is logged through the auth context's logger, and authentication still succeeds normally.
- No `Set-Cookie` for the last-login-method cookie may appear in the response.

**Default**

- When the option is omitted, the cookie is always set, exactly as before.

**Interaction with other options**

- The hook applies to whatever cookie name is configured. With a custom `cookieName`, that name is the cookie that gets set or skipped.
- The hook only controls the cookie. With `storeInDatabase: true`, the user's `lastLoginMethod` field is still updated in the database even when the hook blocks the cookie.
- The hook applies to all flows the plugin tracks, including email sign-in, email sign-up, and OAuth callbacks.

### Example

```ts
lastLoginMethod({
  beforeStoreCookie: async (ctx, method) => hasConsent(ctx) && method !== "github",
});
```
