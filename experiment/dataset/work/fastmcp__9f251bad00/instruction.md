# Add a Hugging Face OAuth provider

FastMCP has no built-in provider for Hugging Face OAuth. Add one at `fastmcp.server.auth.providers.huggingface`, modelled on the existing OAuth-proxy-based providers such as GitHub and Google.

## Module constants

The module must export:

- `HUGGINGFACE_AUTHORIZATION_ENDPOINT = "https://huggingface.co/oauth/authorize"`
- `HUGGINGFACE_TOKEN_ENDPOINT = "https://huggingface.co/oauth/token"`
- `HUGGINGFACE_USERINFO_ENDPOINT = "https://huggingface.co/oauth/userinfo"`
- `HUGGINGFACE_WHOAMI_ENDPOINT = "https://huggingface.co/api/whoami-v2"`
- `DEFAULT_HUGGINGFACE_SCOPES = ["openid", "profile"]`

## `HuggingFaceTokenVerifier(TokenVerifier)`

Signature (keyword-only): `required_scopes: list[str] | None = None, timeout_seconds: int = 10, http_client: httpx.AsyncClient | None = None`.

Hugging Face access tokens are opaque. `async verify_token(token) -> AccessToken | None` must work as follows.

**Userinfo lookup**
- GET the userinfo endpoint with the header `Authorization: Bearer <token>`.
- Return `None` in any of these cases:
  - the response is not 200;
  - the JSON has no `sub`;
  - a network error occurs;
  - the JSON cannot be decoded.

**Scope extraction**
- Read scopes from the `scope` or `scopes` field.
  - A space-separated string is split, preserving order.
  - A list is used as-is.

**Whoami fallback**
- Call the whoami endpoint when either:
  - no scopes were found; or
  - the found scopes do not cover `required_scopes`.
- On a 200 response, merge in scopes from:
  - top-level `scope`/`scopes`; or
  - `auth.accessToken.scopes` (or `.scope`). List items may be strings or objects with a `name`.
- Merge order-preserving and deduplicated, with userinfo scopes first.
- A non-200 whoami response is ignored.

**Defaults and enforcement**
- If there are still no scopes, use `DEFAULT_HUGGINGFACE_SCOPES`.
- If `required_scopes` are not all present, return `None`.

**Returned token**
- `token`: the input token.
- `client_id`: `str(sub)`.
- `scopes`: the resolved scopes.
- `expires_at`: `None`.
- `claims` contains:
  - `sub`, `name`, `email`, `email_verified`, `profile`, `picture`, `organizations` from userinfo;
  - `preferred_username`, taken from userinfo `preferred_username`, falling back to `nickname`, then `name`;
  - `huggingface_userinfo`: the raw userinfo dict;
  - `huggingface_whoami`: the whoami dict, or `None` if whoami was not fetched or failed.

## `HuggingFaceProvider(OAuthProxy)`

**Parameters**
- `client_id` (required).
- `client_secret: str | None = None`. It is optional because public PKCE and CIMD client IDs are supported.
- `base_url` (required).
- The other usual `OAuthProxy` options, such as `resource_base_url`, `issuer_url` (defaults to `base_url`), `redirect_path`, `jwt_signing_key`, `client_storage`, consent options, expiry options, and extra authorize/token params.
- `required_scopes`, `valid_scopes`, `timeout_seconds=10`, `http_client`, `enable_cimd=True`.

**Behaviour**
- Use the Hugging Face authorization and token endpoints as the upstream endpoints. There is no revocation endpoint.
- The default redirect path is the standard `/auth/callback`.
- Token endpoint auth method:
  - `"client_secret_basic"` when a secret is given;
  - otherwise `"none"`, with no upstream secret stored.
- `provider.required_scopes`:
  - defaults to `DEFAULT_HUGGINGFACE_SCOPES` when `required_scopes` is `None`;
  - otherwise equals the parsed `required_scopes`.
- The internal `HuggingFaceTokenVerifier` must be created **without** required scopes, so its `required_scopes == []`. Scope enforcement is left to the proxy, which uses the scopes stored from the upstream token response.
- `valid_scopes` is passed through, so `client_registration_options.valid_scopes` reflects it.
- Default scopes are `valid_scopes` or, if absent, the required scopes.
- `_uses_alternate_verification()` must return `True`, so that returned tokens carry the upstream token-response scopes.
