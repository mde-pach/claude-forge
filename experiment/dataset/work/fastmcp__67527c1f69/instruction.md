# Block unsafe browser schemes in OAuth redirect URIs

## Problem

The OAuth proxy treats `allowed_client_redirect_uris=None` (the default) as "allow any redirect URI" for Dynamic Client Registration compatibility. A client can therefore register a redirect URI such as `javascript:alert(document.cookie)//`. The proxy later sends the user's browser to that URI, which creates an XSS / open-redirect vector. This happens on several paths: after the IdP callback, when forwarding IdP errors, and when the user denies consent. Explicit allowlist patterns such as `javascript:*` can also currently permit these schemes.

## Requirements

**Unsafe schemes.** The schemes `javascript`, `data`, `file` and `vbscript` are unsafe. Matching is case-insensitive, so `JaVaScRiPt:` counts. Expose them as a module-level `frozenset` named `UNSAFE_REDIRECT_URI_SCHEMES` in `fastmcp.server.auth.redirect_validation`. A URI that cannot be parsed is also treated as unsafe.

**`validate_redirect_uri(redirect_uri, allowed_patterns)`** (in `fastmcp.server.auth.redirect_validation`):
- It returns `False` for any URI with an unsafe scheme, whatever the patterns are. This includes `None` and patterns that name the scheme explicitly, such as `javascript:*`, `data:*`, `file:///*` and `vbscript:*`.
- With `allowed_patterns=None`, ordinary URIs are still accepted. Examples are `http://localhost:3000`, `https://app.example.com` and custom app schemes such as `cursor://anysphere.cursor-mcp/oauth/callback`.
- A `redirect_uri` of `None` is still accepted, as before.
- `matches_allowed_pattern` must likewise never match an unsafe-scheme URI.

**`ProxyDCRClient.validate_redirect_uri`** must raise `InvalidRedirectUriError` for unsafe-scheme URIs:
- even when no allowed patterns are configured;
- even when that URI is among the client's registered `redirect_uris`;
- also when the URI is resolved from CIMD defaults.

**`OAuthProxy.register_client`:**
- If any provided redirect URI fails validation against the proxy's `allowed_client_redirect_uris`, it raises `mcp.server.auth.provider.RegistrationError`. The error code is `"invalid_redirect_uri"` and the description is `Redirect URI '<uri>' is not allowed.`. Unsafe schemes are always rejected.
- Ordinary external HTTPS callbacks are still accepted under the default configuration.
- If the client omits `redirect_uris`, registration succeeds and the stored client gets the placeholder `http://localhost`. Allowlist validation is then deferred to authorization time:
  - A later `validate_redirect_uri` with an allowed URI succeeds.
  - Calling it with `None` raises `InvalidRedirectUriError`.

**Redirects from stored transactions.** Before the proxy redirects the browser to a transaction's stored client redirect URI, it must validate that URI against the configured patterns, including the unsafe-scheme check. If validation fails, the proxy must not redirect. It returns an HTTP 400 HTML response with no `Location` header, and the body contains the text `Invalid redirect URI`. This applies to:
- the IdP callback success path. The check happens before any upstream token exchange; no upstream OAuth client is created and no token is fetched.
- the IdP callback when it forwards an `error` back to the client;
- consent denial, both an explicit `deny` action on the consent form and a previously-denied client.
