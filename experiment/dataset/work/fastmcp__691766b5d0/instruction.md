# OpenAPI resource templates build incorrect HTTP requests

When an OpenAPI route is exposed as an MCP resource template (e.g. via `RouteMap(methods=["GET"], mcp_type=MCPType.RESOURCE_TEMPLATE)` on an `OpenAPIProvider`), reading a concrete resource such as `resource://get_user/alice` produces the wrong upstream HTTP request in several ways:

- **Path values are not encoded.** The value is spliced directly into the URL path, so a URI like `resource://get_user/..%2F..%2Fadmin%2Fsecret` can traverse out of the intended path. Path parameter values must be percent-encoded as a single path segment. For a route `/users/{id}` with base URL `https://api.example.com/api/v1`, that read must request exactly `https://api.example.com/api/v1/users/%2E%2E%2F%2E%2E%2Fadmin%2Fsecret`.
- **Values come from the URI's string layout rather than the matched template parameters.** A query string on the resource URI that the template doesn't match, as in `resource://get_user/alice?admin=true`, must not leak into the request. That read must request `https://api.example.com/api/v1/users/alice`.
- **Hyphenated path parameter names break.** A route `/users/{user-id}` read as `resource://get_user/abc` must request `.../users/abc`. Template parameters whose names differ from the OpenAPI name only by `-` versus `_` must still be matched.
- **Client defaults are lost.** Default query params and cookies configured on the `httpx.AsyncClient` passed to the provider must be applied. For example, with `params={"api-version": "2026-06-29"}` and `cookies={"session": "abc123"}`, the request URL must be `.../users/alice?api-version=2026-06-29` and the `cookie` header must be `session=abc123`. Client default headers must also still apply. Headers from the incoming MCP request continue to be layered on top.
- **Renamed arguments are not respected.** When a path parameter's argument name is suffixed because of a name collision, the template must still fill the path correctly. This covers:
  - a request body property also named `id`, giving the argument `id__path`;
  - a query parameter also named `id`.

  In both cases, reading `resource://get_user/abc` must request `.../users/abc`.

Resource reads should build their requests the same way OpenAPI tools do, from the route's parameter mapping, rather than with ad-hoc string substitution.

## Parameter collision handling

`_combine_schemas_and_map_params(route)` in `fastmcp.utilities.openapi.schemas` returns `(schema, param_map)`. It already suffixes parameters that collide between the body and the non-body locations. It must now also suffix parameters whose name appears in more than one non-body location (path, query, header, cookie).

Example: a route has `id` in path (required), in query, and in header.
- The schema properties must contain `id__path`, `id__query` and `id__header`, and must not contain `id`.
- `schema["required"] == ["id__path"]`.
- `param_map["id__path"] == {"location": "path", "openapi_name": "id"}`, and likewise for `id__query` and `id__header`.

## Resource constructor

`OpenAPIResource.__init__` accepts a new optional keyword argument `arguments: dict[str, Any] | None = None`. These are the argument values, keyed by argument name, that are used to build the request when the resource is read.
