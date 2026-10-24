# Azure provider: fall back to configured scopes when the client omits `scope`

## Problem

Some MCP clients start the authorization flow without sending a `scope` parameter. `AzureProvider` already handles this when building the upstream authorize URL by using its configured `required_scopes`. The later steps do not.

- **Authorization-code token exchange:** the scopes sent to Azure contain no API scope. The result is `offline_access` alone, or nothing at all.
- **Upstream refresh-token requests:** the request contains only the OIDC scopes from `additional_authorize_scopes`, such as `openid` and `offline_access`, and no API scope.

Azure needs a resource/API scope in these requests. Without one, the token exchange and refresh fail or return the wrong tokens.

## Required behaviour

When the stored or requested scope list is empty or `None`, `AzureProvider` must use its configured `required_scopes` instead. This applies in both places below, which callers may invoke directly with a list of scopes:

- `AzureProvider._prepare_scopes_for_token_exchange(scopes)`
- `AzureProvider._prepare_scopes_for_upstream_refresh(scopes)`

The fallback scopes go through the same processing as explicitly requested scopes:

- Each scope is prefixed with `identifier_uri`, for example `read` becomes `api://my-api/read`. Scopes that already contain `://` are left unchanged, as before.
- In the refresh path, any `additional_authorize_scopes` entries are filtered out of the base scopes, as before.
- OIDC scopes from `additional_authorize_scopes` are added, and `offline_access` is included automatically, as before.

When `required_scopes` is also unset, the behaviour stays as it is today.

When a non-empty scope list is supplied, the behaviour is unchanged.

## Examples

Both examples use this provider configuration:

- `identifier_uri="api://my-api"`
- `required_scopes=["read"]`

**Token exchange**, with no `additional_authorize_scopes`:

`_prepare_scopes_for_token_exchange([])` must return exactly:

```
["api://my-api/read", "offline_access"]
```

**Upstream refresh**, with `additional_authorize_scopes=["User.Read", "openid"]`:

`_prepare_scopes_for_upstream_refresh([])` must return exactly 3 scopes:

- `api://my-api/read`
- `openid`
- `offline_access`

It must not include `User.Read`, because only OIDC scopes are added.
