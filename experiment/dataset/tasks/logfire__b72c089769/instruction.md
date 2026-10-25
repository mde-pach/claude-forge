You are working in the git repository in the current directory. Make the change described below. Work autonomously: nobody will answer questions.

# Managed variables ignore the API key's region for new-format keys

## Problem

Logfire tokens and API keys carry their region in a prefix of the form `pylf_v<N>_<region>_...`, for example `pylf_v1_eu_...` or `pylf_v2_stagingeu_...`. The SDK uses this region to pick the backend URL.

Region detection currently only works for keys whose suffix matches the exact token format the SDK expects. Newer keys use a different suffix, for example:

```
pylf_v2_stagingeu_9F9BA85A-B759-4181-9527-D812E03F9F7F_0kYhc414Ys2FNDRdt5vFB05xFx5NjVcbcBMy4Kp6PH0W
```

The version is `v2`, and the suffix is a UUID (upper- or lowercase) followed by a long secret. For these keys the region is not recognised, and the SDK silently falls back to the US backend.

Managed (remote) variables are affected in particular. The remote variable provider can end up pointed at the wrong region. This happens both when `variables=VariablesOptions()` is passed to `logfire.configure()` and when the provider is created lazily because `LOGFIRE_API_KEY` is set.

## Required behaviour

1. **Region detection must ignore the suffix format.**
   - `get_base_url_from_token(token: str) -> str` in `logfire._internal.config` must take the region from the `pylf_v<digits>_<lowercase-region>_` prefix only.
   - It must not validate anything after the region segment.
   - Existing mappings must keep working:
     - `stagingus` → `https://logfire-us.pydantic.info`
     - `stagingeu` → `https://logfire-eu.pydantic.info`
     - normal regions resolve via the known region table
     - tokens without a recognisable region default to US
   - Example: `get_base_url_from_token('pylf_v2_stagingeu_9F9BA85A-..._0kYh...')` returns `'https://logfire-eu.pydantic.info'`.

2. **The managed variables provider must use the API key's region.**
   - When a `LogfireRemoteVariableProvider` is created through `logfire.configure(...)`, its base URL must be resolved the same way as the rest of the SDK's API base URL. This applies whether `variables=` is passed explicitly or the provider is created lazily by `config.get_variable_provider()` when `LOGFIRE_API_KEY` is set.
   - Resolution order:
     1. An explicitly configured base URL (advanced options / `LOGFIRE_BASE_URL`) takes precedence.
     2. Otherwise, the base URL is inferred from the API key's region.
   - Example: configuring with `api_key=` set to the `pylf_v2_stagingeu_...` key above, with no explicit base URL, gives a provider whose `_base_url` attribute equals `'https://logfire-eu.pydantic.info'`.
   - The lazy path must give the same result: set `LOGFIRE_API_KEY` to that key, do not pass `variables=`, and call `logfire.DEFAULT_LOGFIRE_INSTANCE.config.get_variable_provider()`. The provider's `_base_url` must equal `'https://logfire-eu.pydantic.info'`.

3. **Existing behaviour must not regress.** Keys in other formats, and explicit base URL overrides, must continue to behave as before.
