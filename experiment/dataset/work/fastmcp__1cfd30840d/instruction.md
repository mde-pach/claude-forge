# `HeadlessOAuth` callback drops explicitly empty query parameters

## Problem

`fastmcp.utilities.tests.HeadlessOAuth` is a test helper that runs an OAuth flow without a browser. Its async `callback_handler()` parses the redirect `Location` header from the stored 302 response. It returns `(auth_code, state)`, or raises `RuntimeError` when the redirect carries an `error` parameter.

When it parses the query string, any parameter whose value is empty is discarded. Two cases go wrong as a result:

- **Empty `state`:** for `https://example.com/callback?code=abc&state=`, the handler returns `state` as `None`. It should return `""`. Real OAuth callbacks can carry an empty `state`, and downstream code treats `""` and "missing" differently.
- **Empty `error_description`:** for `https://example.com/callback?error=invalid_request&error_description=`, the raised error falls back to the generic `"Unknown error"` text. It should use the empty description that was actually sent.

## Required behaviour

The callback handler reads the stored response from the existing `_stored_response` attribute (it has `status_code` and `headers["location"]`). Keep using that attribute. Then:

1. **Blank values are preserved.** Query parameters that are present with an empty value are kept as `""`.
2. **Empty `state`:** for `?code=abc&state=`, `await oauth.callback_handler()` returns `("abc", "")`.
3. **Missing `state`:** for `?code=abc` with no `state` key, it still returns `("abc", None)`.
4. **Errors:** when `error` is present, a `RuntimeError` is still raised, and its message still ends with `"<error> - <error_description>"`.
   - If `error_description` is present but empty, use it verbatim. The message then ends with `"invalid_request - "`, with nothing after the dash and space.
   - The `"Unknown error"` fallback applies only when `error_description` is entirely absent.

Everything else about `HeadlessOAuth` is unchanged.
