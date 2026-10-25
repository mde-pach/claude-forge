You are working in the git repository in the current directory. Make the change described below. Work autonomously: nobody will answer questions.

# Sign-up / sign-in / email verification fail when the request cannot be cloned

## Problem

Several email-and-password flows pass a copy of the incoming `Request` to user-supplied callbacks, so the callback can read the request (including its body) without consuming the original. Those flows are:

- `emailVerification.sendVerificationEmail`, called during sign-up, sign-in, and `verify-email` (including the email-change flow).
- `emailAndPassword.onExistingUserSignUp`.

The copy is made by calling `Request.prototype.clone()` directly. In some runtimes or adapters, `clone()` throws, for example with a `TypeError` when the body stream is already disturbed or locked. When that happens, the whole endpoint fails. A user can end up unable to sign up only because the request passed to the callback could not be cloned.

## Required behaviour

Failing to clone the request must never make these endpoints fail. Callbacks must still receive a usable `Request`.

- If cloning succeeds, the callback receives the clone, as it does today.
- If cloning throws, the callback receives a new, **bodyless** `Request` with these properties:
  - The same `url`, `method` and `headers` as the original.
  - The same other request options where available: `cache`, `credentials`, `integrity`, `keepalive`, `mode`, `redirect`, `referrer`, `referrerPolicy` and `signal`.
  - Reading its body with `text()` resolves to `""`.
- If there is no request (for example, a server-side API call), the callback receives `undefined`, as it does today.

For example, a JSON `POST` to `/api/auth/sign-up/email` with `sendOnSignUp: true` must succeed with status 200 even if `clone()` throws. In that case, `sendVerificationEmail` must still be called with a request that has method `POST`, the original URL, and an empty body.

## Public helper

Add a utility module at `packages/better-auth/src/utils/request.ts` that exports:

```ts
safeCloneRequest(request?: Request): Request | undefined
```

It must behave as follows:

- Return `undefined` when `request` is `undefined`.
- Otherwise return `request.clone()`.
- If `clone()` throws, return the bodyless fallback `Request` described above. The fallback is a new object, never the original request instance.

Use this helper everywhere the flows above currently pass a cloned request to a callback.
