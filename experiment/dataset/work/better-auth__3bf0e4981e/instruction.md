## Organization delete hooks should receive the endpoint context

### Problem

The organization plugin lets callers register lifecycle hooks via `organizationHooks` in the `organization({...})` options. Most of these hooks receive the endpoint context as a second argument. That lets user code inspect the incoming request, headers and so on.

`beforeDeleteOrganization` and `afterDeleteOrganization` do not get it. When an organization is deleted through the delete-organization endpoint (for example, `client.organization.delete({ organizationId })`), both hooks are called with only the data object. Code that needs request information during deletion cannot get it. Examples are audit logging and reading headers.

### Required behaviour

- The type of both hooks under `OrganizationOptions["organizationHooks"]` must accept an optional second parameter, the generic endpoint context (`GenericEndpointContext`):

  ```ts
  beforeDeleteOrganization?: (
    data: { organization: Organization & Record<string, any>; user: User & Record<string, any> },
    ctx?: GenericEndpointContext,
  ) => Promise<void>;
  afterDeleteOrganization?: (same signature) => Promise<void>;
  ```

  Existing hooks that take only `data` must still type-check and work.

- When an organization is deleted through the delete-organization endpoint, both hooks must be called with the endpoint context as the second argument:
  - The context must be defined.
  - When the call came in as an HTTP request through `auth.handler`, `ctx.request` must be populated.
  - The first argument stays the same: `{ organization, user }`.

- The Stripe plugin (`@better-auth/stripe`) wraps a user-supplied `beforeDeleteOrganization` hook with its own logic. That wrapper must forward the context argument to the user's hook, so the user's hook still receives `ctx` when the Stripe plugin is enabled.
