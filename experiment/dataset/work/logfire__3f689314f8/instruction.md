# Resource `process.pid` is stale in forked child processes (especially for metrics)

## Problem

When a process using Logfire forks (for example, gunicorn or multiprocessing workers), telemetry from the child should carry the child's PID in the `process.pid` resource attribute. The current after-fork handling is broken in several ways:

- **Metrics** keep reporting the parent's PID. The SDK meter provider's resource is not stored where the fork handler writes it.
- **Spans and logs** are only partly fixed. Tracers and loggers created before the fork keep their own reference to the old resource, so their records still carry the parent PID.
- The handler is registered on every `configure()` call, so handlers pile up. Each one also holds strong references to old providers.
- The handler can **deadlock** the child. If another thread held a provider's internal lock at the moment of the fork, that lock stays held in the child. Some OpenTelemetry versions have lock-taking resource updaters, and calling them in the child would block. The fork callback runs before OpenTelemetry resets its own locks.

## Requirements

Add a module-level function in `logfire._internal.config`:

```python
_register_at_fork_resource_updates(proxy_tracer_provider, proxy_meter_provider, proxy_logger_provider) -> None
```

### Registration

- The function takes Logfire's proxy tracer, meter and logger providers.
- If `os.register_at_fork` exists, call it exactly once, as `os.register_at_fork(after_in_child=callback)`. Look up `os` through the `logfire._internal.config` module.
- If `os.register_at_fork` is not available, do nothing.
- `LogfireConfig.__init__` must call this function once, with its own proxy providers. It must not be called on each `configure()`.
- The callback must hold the proxies only weakly. If any of the three has been garbage-collected, the callback does nothing.

### The child callback

The callback must use the current `os.getpid()`, looked up via the config module's `os`. It must update the resource with a merged `process.pid` resource, so that:

- **Tracing:** if the proxy's current underlying provider is an SDK `TracerProvider`, update its resource. Also update the resource of every SDK tracer already handed out through the proxy, so new spans report the new PID.
- **Metrics:** if the underlying provider is an SDK `MeterProvider`, update its resource so exported metrics report the new PID. This includes metrics from instruments created before the fork.
- **Logging:** if the underlying provider is an SDK `LoggerProvider`, update its resource. Also update every SDK logger already handed out through the proxy, so new log records report the new PID.

The callback must also meet these constraints:

- It must never acquire provider locks. It must not deadlock if those locks were held at fork time.
- No-op providers must be skipped without error. This applies to each of the three providers, independently.
- Tracers and loggers from suppressed scopes (which are not SDK instances) must be skipped without error.
- Internal errors must be handled using Logfire's usual internal-error handling, and must not propagate.

## Expected outcome

After a real `os.fork()`, any span, log record or metric export produced in the child has `resource.attributes['process.pid']` equal to the child's PID. This holds for tracers, loggers and instruments created before the fork, and also when provider locks were held at fork time.
