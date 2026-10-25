You are working in the git repository in the current directory. Make the change described below. Work autonomously: nobody will answer questions.

# Add server-level cache hints (SEP-2549) to `FastMCP`

## Problem

Clients that support response caching (e.g. `fastmcp.Client(cache=True)` negotiating protocol `2026-07-28`) only cache results that carry a TTL hint (`ttlMs`, optionally `cacheScope`). A `FastMCP` server has no way to emit these hints, so its listings and resource reads are never cached client-side.

## Requirements

### Constructor parameters

`FastMCP(...)` gains two keyword arguments:

- `cache_ttl: int | None = None`: TTL in **seconds**.
- `cache_scope: Literal["public", "private"] | None = None`.

Behaviour:

- With no `cache_ttl`, the server emits no hint. Its wire output is unchanged from today.
- With `cache_ttl` set, every SDK-cacheable result carries the same hint:
  - The cacheable results are `tools/list`, `prompts/list`, `resources/list`, `resources/templates/list`, `resources/read` and `server/discover`.
  - `ttl_ms` is `cache_ttl * 1000`.
  - `cache_scope` is the given scope, defaulting to `"private"`.
- The hint is uniform and server-wide. There is no per-component configuration.
- Rely on the low-level SDK `Server`'s cache-hint support (its `cache_hints=` parameter) to fill the wire fields. Do not hand-set them. Fields a handler sets explicitly must be left untouched.
- Validation happens at construction time and raises `ValueError` in two cases:
  - `cache_ttl` is ≤ 0. The message must contain `cache_ttl must be a positive integer`, e.g. `cache_ttl must be a positive integer, got 0`.
  - `cache_scope` is given without `cache_ttl`. The message must contain `cache_scope requires cache_ttl`.

### Public helper

Add a new module `fastmcp.server.caching` exposing:

- `CacheScope = Literal["public", "private"]`
- `build_cache_hints(cache_ttl: int | None, cache_scope: CacheScope | None) -> dict[CacheableMethod, CacheHint] | None`

`CacheHint` comes from `mcp.server.caching` and `CacheableMethod` comes from `mcp_types.methods`.

`build_cache_hints` behaves as follows:

- It returns `None` when `cache_ttl` is `None` and no scope is given.
- Otherwise it returns a dict whose keys are exactly the SDK's cacheable methods (`mcp_types.methods.CACHEABLE_METHODS`). Each key maps to `CacheHint(ttl_ms=cache_ttl * 1000, scope=cache_scope or "private")`.
- It raises the same `ValueError`s, with the same messages, as described for the constructor.

### Expected outcome

Example:

```python
mcp = FastMCP("x", cache_ttl=60, cache_scope="public")
```

For this server, the following hold:

- `client.session.list_tools()` returns a result with `ttl_ms == 60000` and `cache_scope == "public"`. The same applies to prompts, resources, resource templates and `read_resource`.
- With `Client(mcp, cache=True)`, a second `client.list_tools()` is served from the cache with no second wire request.
- An unhinted server still triggers a wire request on every call.
