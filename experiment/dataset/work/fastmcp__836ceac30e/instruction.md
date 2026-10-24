# Add a key-value-backed response cache store for distributed client caching

## Problem

The MCP SDK's client response cache reads and writes through a pluggable `ResponseCacheStore` protocol (`get`/`set`/`delete`/`clear`), passed as `mcp.client.caching.CacheConfig(store=...)`. The default store is a per-client in-memory LRU. That means several FastMCP clients, such as a set of proxy replicas, cannot share cached responses. FastMCP already uses the `AsyncKeyValue` abstraction (`key_value.aio`) for its other state, but nothing connects it to the response cache.

## Requirements

Add a new module `fastmcp.client.caching` exposing the following:

- **`DEFAULT_CACHE_COLLECTION = "fastmcp_response_cache"`**

- **`CACHEABLE_RESULT_MODELS: dict[str, type]`**
  - Maps class names to result model classes.
  - Covers exactly the SDK's cacheable result types: `DiscoverResult`, `ListPromptsResult`, `ListResourceTemplatesResult`, `ListResourcesResult`, `ListToolsResult`, `ReadResourceResult`.
  - Derive it from the SDK's method/result registry rather than hard-coding it.

- **`_CacheEnvelope`**
  - A pydantic model used as the serialized form of a cache entry.
  - Fields: `type_tag: str` (the result class name), `value_json: str`, `scope: str`, `expires_at: float | None`.

- **`KeyValueResponseCacheStore(storage: AsyncKeyValue | None = None, *, collection: str = DEFAULT_CACHE_COLLECTION)`**
  - Implements the SDK store protocol.
  - `storage` defaults to an in-process `MemoryStore`.
  - Exposes a `_adapter` attribute: a pydantic key-value adapter over `_CacheEnvelope` whose default collection is `collection`. It supports `put(key=..., value=...)`.
  - Provides a static method `_string_key(key: CacheKey) -> str`.

### Behaviour of `KeyValueResponseCacheStore`

**Keys**
- `_string_key` derives a stable string from all three `CacheKey` fields: `method`, `params_key` and `partition`.
- The encoding must be unambiguous, for example by length-prefixing each field and then hashing.
- Entries that differ in any field must never collide.

**`set`**
- Stores a `CacheEntry` whose value is an allowlisted cacheable result.
- The result's JSON (by alias), `scope` and `expires_at` are stored in an envelope.
- If `expires_at` is in the future, the backend TTL is the remaining seconds; otherwise no TTL is set.
- Values that are not allowlisted results are silently ignored.

**`get`**
- Returns `None` on a miss.
- Otherwise returns a `CacheEntry` whose value is reconstructed as an instance of the original model class and equals the original.
- `scope` (`"public"`/`"private"`) and `expires_at` must be preserved.
- If the stored envelope's `type_tag` is not in the allowlist, return `None`. Never import a type by name.

**`delete`**
- Removes the entry.

**`clear`**
- Removes only this instance's collection. Other adapters on the same backend with a different `collection` are unaffected.
- The store must remain usable afterwards.
- Prefer enumerating and deleting keys, and fall back to destroying the collection.
- If the backend supports neither, do nothing and log a warning once.

### Integration

These must work with the store:

- `CacheConfig(store=KeyValueResponseCacheStore(), partition="p", target_id="srv")` can be passed to `fastmcp.Client(..., mode="auto", cache=config)`, and the client builds a response cache.
- Existing validation errors are kept:
  - FastMCP's `"requires CacheConfig.target_id"` error when no `target_id` is given.
  - The SDK's `"requires an explicit partition"` error when no partition is given.
- Two independent clients that share one store and the same partition/target share cache entries. The second client's first `list_tools()` is served from the cache without calling the session.
- Clients with different partitions do not share entries.

Document the distributed response cache store in the user-facing docs.
