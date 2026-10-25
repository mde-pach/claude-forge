You are working in the git repository in the current directory. Make the change described below. Work autonomously: nobody will answer questions.

## Make HTTP Host/Origin protection opt-in, with a new `"auto"` mode

### Problem

HTTP apps now validate the `Host` and `Origin` headers by default. This breaks existing deployments behind reverse proxies and on public hostnames, which suddenly get `421 Misdirected Request` or `403 Forbidden Origin`. The guard must be off by default. Users also need an `"auto"` mode that protects localhost-bound servers and explicit allowlists without breaking ambiguous deployments.

### Requirements

**Settings**
- `Settings.http_host_origin_protection` has type `bool | Literal["auto"]` and defaults to `False`.
- The env var `FASTMCP_HTTP_HOST_ORIGIN_PROTECTION` parses `"auto"` → `"auto"`, `"true"` → `True` and `"false"` → `False`.

**`create_streamable_http_app(..., host_origin_protection=False, ...)`** (in `fastmcp.server.http`)
- The parameter accepts `True`, `False` or `"auto"`, and defaults to `False`.
- Any other value raises `ValueError("host_origin_protection must be True, False, or 'auto'.")`.
- `False`: no guard is installed. Requests pass even if `allowed_hosts` is given.
- `True`: the guard runs in `"strict"` mode.
- `"auto"`: the guard runs in `"auto"` mode.
- `fastmcp.server.http` exports the type alias `HostOriginProtection = bool | Literal["auto"]`.

**`HostOriginGuardMiddleware(app, allowed_hosts=None, allowed_origins=None, mode="auto")`**
- `mode` is `"auto"` or `"strict"`.
- Rejections keep the existing responses: `421 "Misdirected Request"` for a bad Host and `403 "Forbidden Origin"` for a bad Origin.
- "Explicit" below means the argument was passed as something other than `None`.

**When the Host header is validated**
- Always in strict mode.
- Always when `allowed_hosts` is explicit.
- Otherwise, only when the ASGI scope's `server` address is a loopback host. If `server` is missing or not loopback, the Host is not checked.

**When the Origin header is validated** (only if an `Origin` header is present)
- In strict mode.
- When `allowed_hosts` or `allowed_origins` is explicit.
- When the Host header is loopback.
- When the scope `server` is loopback.
- Otherwise it is not checked.

**Same-origin / loopback fallback**
- An origin matching `allowed_origins` is always accepted.
- The existing fallback (same-origin, or loopback-to-loopback) is allowed in these cases:
  - `allowed_origins` is not explicit;
  - strict mode;
  - `allowed_hosts` is explicit;
  - the Host header or the scope `server` is loopback.
- Otherwise the origin must match `allowed_origins`.
  - Example: auto mode, `server=None`, `allowed_origins=["https://app.example.com"]`, Host `attacker.example`, Origin `https://attacker.example` → `403`.

**Run helpers** (`fastmcp.server.mixins.transport`)
- `run_http_async` and `http_app` accept `host_origin_protection: HostOriginProtection | None`.
- `run_http_async` resolves `None` from settings.
- Add `_resolve_allowed_hosts_for_run(*, host, host_origin_protection, allowed_hosts, configured_allowed_hosts) -> list[str] | None`:
  - If `allowed_hosts` is not `None`, return it unchanged.
  - Else, if protection is `"auto"` and `host` is loopback, return `[*(configured_allowed_hosts or []), host]`.
  - Else return `configured_allowed_hosts`.
- `run_http_async` uses this helper with `settings.http_allowed_hosts`, and passes the resolved protection value and host list to the app.
