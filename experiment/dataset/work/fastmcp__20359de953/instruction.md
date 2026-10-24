## Blank query parameter values are dropped when matching resource URI templates

### Problem

When a resource URI template declares query parameters (RFC 6570 form-style, e.g. `resource://data/{id}{?format}`), a URI that supplies a parameter with an **empty value** loses that parameter entirely.

For example, matching `resource://data/42?format=` against `resource://data/{id}{?format}` returns `{"id": "42"}`. The `format` key is missing. The caller therefore cannot tell "the client explicitly sent an empty `format`" apart from "the client did not send `format` at all". This loses caller intent. It also causes function defaults to be applied where the client asked for an empty string.

### Required behaviour

`fastmcp.resources.template.match_uri_template(uri: str, uri_template: str) -> dict[str, str] | None` must keep query parameters whose value is blank. They should appear in the result as an empty string `""`.

- `match_uri_template("resource://data/42?format=", "resource://data/{id}{?format}")` returns `{"id": "42", "format": ""}`.
- Blank and non-blank values may be mixed. `match_uri_template("resource://data/42?format=&verbose=true", "resource://data/{id}{?format,verbose}")` returns `{"id": "42", "format": "", "verbose": "true"}`.
- Query parameters that are absent from the URI remain absent from the result, exactly as before.
- Path parameter matching, the handling of query parameters not declared in the template, and the return of `None` on non-matching URIs are all unchanged.

### Related: token bucket rate limiter must not let denied retries accrue extra tokens

Verify that `TokenBucketRateLimiter(capacity, refill_rate)` (in `fastmcp.server.middleware.rate_limiting`) refills strictly according to `refill_rate`, regardless of how many `consume()` calls are denied. Fix it if it does not.

- Every `await limiter.consume(n)` call must account for elapsed time only once. This holds whether the call succeeds or fails.
- Rapidly retrying after a denial must not cause the same elapsed interval to be counted again.

For example, take a limiter with `capacity=10` and `refill_rate=10.0`:

1. Drain it with `consume(10)`.
2. Make about 20 denied `consume(1)` calls spread over roughly 0.2 seconds.

After that, the bucket should hold about 2 tokens, so `consume(5)` must return `False`.
