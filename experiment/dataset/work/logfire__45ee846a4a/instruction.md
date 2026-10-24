## `logfire_api.instrument` fails when used as a bare decorator (no-op fallback)

### Problem

The `logfire-api` package provides a lightweight, no-op stand-in for the `logfire` SDK. It is used when the real `logfire` package is not installed. Libraries can then add instrumentation without a hard dependency on `logfire`.

The real `logfire.instrument` can be used in two ways:

```python
@logfire.instrument
def f(): ...

@logfire.instrument('Doing {x}')
def g(x): ...
```

The no-op fallback in `logfire_api` only handles the second form, the decorator factory. Bare usage like the one below breaks when `logfire` is not installed:

```python
import logfire_api

@logfire_api.instrument
def bare_instrumented() -> str:
    return 'ok'
```

In the bare form, the decorated function is passed in as the first argument. The fallback returns its inner decorator instead of the function. As a result, `bare_instrumented` is no longer the original function. Calling `bare_instrumented()` does not return `'ok'`, and the function's identity, including its `__name__`, is lost.

### Required behaviour

When the real `logfire` package is unavailable and the no-op fallback is in use, `logfire_api.instrument`, and the same method on the fallback `Logfire` instance, must support both call styles:

- **Bare decorator** (`@logfire_api.instrument`): returns the decorated function itself, unchanged.
  - Calling the result behaves exactly like the original. In the example above, `bare_instrumented()` returns `'ok'`.
  - Its `__name__` is preserved, so `bare_instrumented.__name__ == 'bare_instrumented'`.
- **Decorator factory** (`@logfire_api.instrument(...)`, with or without a message template and keyword arguments): keeps working as before. It returns a decorator that hands back the decorated function unchanged.

The first positional parameter of the fallback `instrument` should be `msg_template` (default `None`). Other options are accepted as keyword arguments and ignored. When `msg_template` is callable, treat the call as the bare-decorator form.

The behaviour when the real `logfire` package is installed must not change.
