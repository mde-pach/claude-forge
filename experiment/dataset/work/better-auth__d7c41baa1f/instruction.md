## `/electron/init-oauth-proxy` merges multiple `Set-Cookie` headers into one

### Problem

The Electron plugin's `/electron/init-oauth-proxy` endpoint (exposed as `auth.api.electronInitOAuthProxy`) starts a social sign-in on the user's behalf. It then forwards the cookies from that inner sign-in response to the client.

Today, if the inner response sets more than one cookie, the endpoint returns them as a single comma-joined `Set-Cookie` header. Browsers and HTTP clients cannot reliably split that header back into separate cookies. As a result, cookies such as the OAuth state cookie and other auth cookies are lost or corrupted.

### Expected behaviour

- Each cookie set by the inner sign-in response must appear in the endpoint's response as its own `Set-Cookie` header. Calling `response.headers.getSetCookie()` on the result must return one entry per cookie.
- The cookies must keep the order in which the inner response emitted them.
- Each forwarded header must begin with `name=value` for its cookie. For example, given an inner response with:
  - `first.cookie=one; Path=/; HttpOnly`
  - `second.cookie=two; Path=/; HttpOnly`

  the endpoint must return exactly two `Set-Cookie` headers. The first starts with `first.cookie=one` and the second with `second.cookie=two`.
- Cookie attributes must be preserved when forwarding: `Path`, `HttpOnly`, `Secure`, `SameSite`, `Max-Age`, `Expires` and `Domain`.
- Existing behaviour must stay the same:
  - When the inner response contains a redirect URL with `redirect: true`, the endpoint still responds with `302` and a `Location` header.
  - When the inner response sets no cookies, no `Set-Cookie` header is added.

The endpoint must work when called with `asResponse: true` and query parameters `provider`, `state`, `code_challenge` and `code_challenge_method`. The inner sign-in request is made with the global `fetch`.
