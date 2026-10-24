# DCR clients accept any redirect URI when no redirect patterns are configured

## Problem

When an `OAuthProxy` (or `OIDCProxy`) is created without `allowed_client_redirect_uris` (the default, `None`), dynamically registered clients are not restricted to the redirect URIs they registered. A client that registered `http://localhost:6274/oauth/callback` can start an authorization with `http://evil.com/anything` and have it accepted. This allows authorization codes to be redirected to arbitrary destinations.

## Required behaviour

The change applies to `ProxyDCRClient.validate_redirect_uri(redirect_uri)` in `fastmcp.server.auth.oauth_proxy.models`. This is the client object returned by `OAuthProxy.get_client(...)` after `register_client(...)`.

### No proxy patterns configured (`allowed_redirect_uri_patterns is None`)

A requested redirect URI is accepted if it matches one of the client's registered `redirect_uris` under either of these rules:

- **Exact match.** For example, a registered `https://client.example.com/oauth/callback` is accepted only exactly as registered. A different port such as `:8443`, or a different host, is rejected.
- **Loopback match.** This applies when the registered URI's host is a loopback host: `localhost`, `127.0.0.1` or `::1`, compared case-insensitively. Only the **port** may differ. All of the following must match the registered URI:
  - scheme
  - host (it must be the same host, so `localhost` does not match `127.0.0.1`)
  - path (an empty path is treated as `/`)
  - params, query and fragment

  If either URI contains userinfo (a username or password), the loopback match fails.

  Example: registered `http://localhost:3000/callback` accepts `http://localhost:8080/callback`. It rejects `http://localhost:8080/other`, `http://127.0.0.1:3000/callback` and `http://example.com/callback`.

Anything else raises `mcp.shared.auth.InvalidRedirectUriError` with the message:

`Redirect URI '<uri>' not registered for client`

A client with no registered redirect URIs accepts nothing under these rules.

### Proxy patterns configured

When `allowed_redirect_uri_patterns` is set, a non-matching URI raises `InvalidRedirectUriError("Redirect URI '<uri>' does not match allowed patterns.")`. There is no fallback to the registered URIs. This is unchanged in effect. An empty list still blocks everything.

### Opt-in permissive mode

Add a new field to `ProxyDCRClient`:

- `allow_unregistered_redirect_uris: bool`
- Default: `False`
- Excluded from serialization.

When it is `True` and no proxy patterns are configured, any redirect URI is accepted. The proxy's synthetic fallback client, which is created for unknown client IDs, must set this to `True` so that its existing permissive behaviour is preserved.

### Unchanged behaviour

- The proxy still stores `None` for `allowed_client_redirect_uris` by default.
- CIMD client validation is unaffected.

### Documentation

Update the `allowed_client_redirect_uris` docstrings on `OAuthProxy` and `OIDCProxy`. They should say that with `None`, DCR clients are limited to their registered redirect URIs, with loopback ports allowed to vary.
