## Outbound HTTP requests to the Logfire API can hang forever

Several HTTP requests the Logfire SDK/CLI makes to the Logfire backend are sent without any timeout. If the server accepts the connection but never responds, commands such as `logfire auth` or project and token management commands hang indefinitely.

### What must change

Every outbound request made through the `requests` session in these places must pass an explicit `timeout` (in seconds).

**Device authorization flow** (`logfire._internal.auth`)

- `request_device_code(session, base_api_url)` must send its `POST` to `<base_api_url>/v1/device-auth/new/` with `timeout=15`.
  - Its behaviour is otherwise unchanged. It still returns the tuple `(device_code, frontend_auth_url)` taken from the JSON response.
- `poll_for_token(...)` already uses a 15-second timeout for its `GET` requests. It must continue to do so.
- Both halves of the device flow must use the same timeout value, 15 seconds, defined in one place so the two cannot drift apart.
  - The `wait` endpoint blocks server-side for about 10 seconds, so 15 seconds is a deliberately snug bound.

**API client** (`logfire._internal.client.LogfireClient`)

- Every request the client sends must carry `timeout=30`. This covers all three HTTP methods:
  - **GET:** the request used by, for example, `get_user_information()` against `/v1/account/me`.
  - **POST:** the request used by, for example, `create_write_token(organization, project)` against `/v1/organizations/{org}/projects/{project}/write-tokens/`.
  - **PUT:** the request issued by the client's internal PUT helper, `_put_raw(endpoint, body=None)`. It has no public caller yet, but it must also send the 30-second timeout when invoked directly.
- The timeout must be passed explicitly on each request, so that it is visible on the outgoing request (for example, as `request.timeout` when the transport is mocked with `requests_mock`).

### Acceptance criteria

- A mocked `POST` to the device-code endpoint made via `request_device_code(requests.Session(), 'https://logfire-us.pydantic.dev')` returns the expected tuple, and the recorded request has a timeout of `15`.
- With a `LogfireClient` built from a non-expired `UserToken`, issuing a GET, a POST and a PUT records three requests, each with a timeout of `30`.
- No existing behaviour changes: error handling and return values stay the same, and so do the existing raised exceptions such as `LogfireConfigError` and `RuntimeError` for expired tokens.
