# User token display mishandles newer token formats

## Problem

`logfire._internal.auth.UserToken` has a string form (`str(token)`) used when listing or choosing stored user tokens. It shows the region, the base URL, and a masked prefix of the token. Today this only works for tokens that match the full current Pydantic Logfire token layout: `pylf_v<N>_<region>_[<org-uuid>_]<secret>`.

Tokens issued in newer formats still start with the `pylf_v<N>_<region>_` prefix, but the rest may be structured differently. For example, `pylf_v3_eu_new-token-format` is such a token. When a token like this does not match the full pattern, two things go wrong:

- The region is ignored and the display falls back to `US`.
- The first 5 characters of the raw token are shown (e.g. `pylf_****`), which tells the user nothing useful.

## Required behaviour

`str(UserToken(...))` must produce `"<REGION> (<base_url>) - <visible part>****"`, following these rules in order:

1. **Full token pattern matches.** Behaviour is unchanged. The region comes from the token, and the visible part is the "safe" prefix (including the org UUID, if present) followed by the first 5 characters of the secret.

2. **Only the `pylf_v<digits>_<lowercase-letters>_` prefix matches.**
   - Take the region from that prefix.
   - The visible part is the 5 characters immediately after the prefix.
   - Example: base URL `https://logfire-eu.pydantic.dev` with token `pylf_v3_eu_new-token-format` must render as:
     `EU (https://logfire-eu.pydantic.dev) - new-t****`

3. **Neither matches.** The region is `US`, and the visible part is the first 5 characters of the token. This is unchanged.

In all cases, a region that is not one of the known regions falls back to `US`.

## Consistency with base-URL detection

The prefix-based region detection used for display must be the same one used when deriving the base URL from a token (`get_base_url_from_token` in `logfire._internal.config`). Region detection there depends only on the `pylf_v<N>_<region>_` prefix. The rest of the token can be arbitrary, malformed content.

For example, both of these tokens must resolve to `https://logfire-eu.pydantic.info`:

- `pylf_v2_stagingeu_<valid uuid>_<secret>`
- `pylf_v2_stagingeu_<garbage with symbols and spaces>_<secret>`

The compiled prefix regex `LOGFIRE_TOKEN_REGION_PATTERN` must be importable from `logfire._internal.auth`. It must have a named group `region`. `logfire._internal.config` must continue to expose the same object under that name.
