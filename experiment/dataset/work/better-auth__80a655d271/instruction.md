## Admin client: reactive session is not refreshed after impersonating a user

### Problem

When using the admin plugin on the client (`adminClient()` from the admin plugin's client entry), calling `authClient.admin.impersonateUser({ userId })` successfully switches the server-side session to the impersonated user. However, the reactive session store exposed by the client (`authClient.useSession`, a nanostores-style atom with `get()` and `subscribe()`) keeps returning the admin's session. A fresh `authClient.getSession()` call returns the impersonated user, but subscribers to `useSession` never see the change unless they manually refetch.

UI code bound to `useSession` keeps showing the admin as the current user after impersonation starts.

### Required behaviour

- After a successful `authClient.admin.impersonateUser(...)` call in a browser-like environment (where `window` is defined), the client must automatically trigger a session revalidation. `useSession` subscribers must then receive state whose `data.user.id` is the impersonated user's id, without any manual refetch.
- `authClient.admin.stopImpersonating()` must also trigger the same session revalidation, so `useSession` goes back to the admin's session.
- `impersonateUser` must be sent as an HTTP `POST` request to `/admin/impersonate-user`, in the same way as `stopImpersonating` (`POST /admin/stop-impersonating`).
- Other admin client endpoints must keep their current behaviour. For example, `listUsers` still uses `GET`.

### Acceptance

In a browser-like client where the admin is signed in and `useSession` has loaded the admin's session:

1. Call `admin.impersonateUser({ userId: target })`.
2. Persist the response cookies.
3. A `useSession` subscriber should, shortly after, receive non-pending, non-refetching state whose `data.user.id === target`.
