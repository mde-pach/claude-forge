# Query clients should raise a proper error on unexpected HTTP responses

## Problem

When the Logfire query API returns a status code other than 200, 400 or 422, the query clients (`LogfireQueryClient` and `AsyncLogfireQueryClient`) fail with a bare `AssertionError`. Examples are a `5xx` server error, a `404`, a non-followed `3xx` redirect, a `204 No Content`, or an error page from a proxy. These responses can happen in production, so callers should get a meaningful exception they can catch. Code run with assertions disabled (`python -O`) is worse off: it skips the check and tries to parse the bad response as a result.

## Requirements

- Add a new exception class `UnexpectedResponseError`, a subclass of `RuntimeError`.
  - It must be importable from `logfire.query_client`.
  - It must be listed in that module's `__all__`.
  - It must be defined alongside the existing errors in `logfire.experimental.query_client`.
- Both the sync and async clients must raise `UnexpectedResponseError` for any response status other than 200, 400 or 422. This applies to:
  - all query methods, such as `query_json_rows`;
  - `info()`.
- The exception message must include:
  - the numeric status code;
  - the raw response body text.

  Suggested format:
  `Unexpected response status code 503: 'unavailable'`
- The body of an unexpected response must not be decoded as JSON, even if its `content-type` says `application/json`. Such bodies may be truncated or malformed. For example, a `500` response with content type `application/json` and body `{"detail": "trunca` must still raise `UnexpectedResponseError`, and the message must contain that raw text.
- Existing behaviour for 400 and 422 must be preserved:
  - **Exception types:**
    - 400 raises `QueryExecutionError`.
    - 422 raises `QueryRequestError`.
  - **Exception argument:** the exception's single positional argument (`exc.args == (data,)`) depends on the response's media type:
    - If the media type is `application/json`, the argument is the decoded JSON body.
    - Otherwise, the argument is the response text.
  - **Media type matching:** the check must ignore case and parameters. For example, `APPLICATION/JSON; charset=utf-8` counts as JSON.
  - **Missing header:** if there is no `content-type` header, treat the body as `text/plain`.
- The behaviour must be identical for `LogfireQueryClient` and `AsyncLogfireQueryClient`. This includes clients built with a custom httpx `transport=` keyword argument passed through to the underlying httpx client.
