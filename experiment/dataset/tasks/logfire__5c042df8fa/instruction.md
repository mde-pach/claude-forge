You are working in the git repository in the current directory. Make the change described below. Work autonomously: nobody will answer questions.

# Managed variables should be shared across `with_settings()` siblings

## Problem

Managed variables (created with `Logfire.var(...)` and `Logfire.template_var(...)`) are currently tracked per `Logfire` instance. Instances derived with `with_settings(...)` share the same configuration as their parent, but each one keeps its own separate set of registered variables. This leads to inconsistent behaviour:

- `variables_get()` on one instance does not return variables registered through a `with_settings()` sibling.
- The same variable name can be registered twice, once on the parent and once on a sibling, with no error.
- `variables_clear()` only clears the instance it is called on.
- Code-default composition (`@{name}@` references inside a variable's default value) cannot resolve variables registered on a sibling instance.
- `variables_push()`, `variables_validate()` and building a variables config with `variables=None` only see the calling instance's variables.

## Requirements

Variable registration must be scoped to the shared `LogfireConfig`. All `Logfire` instances that use the same config, including the default `logfire` module-level instance and anything returned by `with_settings()`, must use one registry.

- **`variables_get()`** must return every variable registered on any instance sharing the config, as a list in registration order. It must return the same result whichever sibling it is called on.
  - Example: register `feature_a` via `logfire.var(...)`, then `feature_b` via `logfire.with_settings(tags=['other']).var(...)`. Both `logfire.variables_get()` and the sibling's `variables_get()` must return `[var_a, var_b]`.
- **Duplicate names** must be rejected across siblings. Registering a name that is already registered on any instance sharing the config must raise `ValueError`. The message must contain:
  `A variable with name '<name>' has already been registered`
- **`variables_clear()`** called on any one instance must clear the registry for every instance sharing that config. Afterwards, `variables_get()` must return `[]` on all siblings.
- **Code-default composition** must resolve references to variables registered on sibling instances.
  - Example: after `lf.var(name='greeting', default='Hello', type=str)` and `lf2.var(name='main', default='@{greeting}@ World', type=str)` (with `lf2 = lf.with_settings(...)`), calling `main.get()` must give value `'Hello World'`. Its `composed_from` must contain a single entry with name `'greeting'` and value `'Hello'`.
- When `variables=None` is passed to `variables_push()`, `variables_validate()` or the config-building method, it must operate on all variables in the shared config registry.

Instances with *different* configs, such as separate `logfire.configure(...)` results, must keep separate registries. Update the docstrings of the affected methods to say that they act on variables registered with the instance's config, including `with_settings()` siblings.
