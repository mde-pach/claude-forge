## `/get-session` responses can be cached by browsers and intermediaries

### Problem

The `/get-session` endpoint returns user-specific data: the current session and user, or `null` when no one is signed in. Its responses currently carry no cache-control headers. Browsers, CDNs or proxies may therefore cache them, which causes two kinds of failure:

- A stale session (or a stale `null`) is served after sign-in or sign-out.
- One user's session data could be served to another user.

### Required behaviour

1. **Every response from `/get-session` must be non-cacheable.** This applies whether or not the request is authenticated and regardless of HTTP method. The response must include:
   - `Cache-Control: no-store`
   - `Pragma: no-cache`

   Specifically:
   - An authenticated `GET /api/auth/get-session` returns `200` with the session/user body, and its `cache-control` header contains `no-store`.
   - An unauthenticated `GET /api/auth/get-session` returns `200` with a `null` body, and its `cache-control` header also contains `no-store`.

2. **These headers must not leak onto other endpoints.** Many endpoints and middlewares resolve the current session internally, for example any endpoint whose `use` array contains `sessionMiddleware`, `freshSessionMiddleware` or similar. When they do, headers produced by the session lookup are forwarded onto the outer endpoint's response, and `Set-Cookie` in particular must keep being forwarded as it is today.

   The new `Cache-Control` and `Pragma` headers must **not** be forwarded this way. Match the header name case-insensitively when excluding them.

   For example, a plugin defines a `GET` endpoint created with `createAuthEndpoint` that uses `freshSessionMiddleware` and returns `{ status: true }`. Called with a valid session cookie, it must respond `200` with no `cache-control` header at all, since `res.headers.get("cache-control")` must be `null`.

3. **Nothing else changes.** Other headers the session lookup sets keep reaching the calling endpoint's response as before, including refreshed session cookies.
