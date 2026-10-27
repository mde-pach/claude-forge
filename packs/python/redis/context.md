redis-py. Iterate keys with `scan_iter(match=...)`, never `KEYS`; asyncio clients (`redis.asyncio`) are closed with `await r.aclose()`.
