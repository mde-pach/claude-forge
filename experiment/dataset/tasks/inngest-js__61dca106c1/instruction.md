You are working in the git repository in the current directory. Make the change described below. Work autonomously: nobody will answer questions.

## Mark the tracer provider created by Inngest's OTel instrumentation

### Problem

When `@inngest/otel` sets up tracing, it may create and register its own global OpenTelemetry tracer provider. If the application already had a provider registered, that one is used as-is. Today there is no way for other code to tell these two cases apart. Code that inspects the global tracer provider cannot know whether Inngest created it or whether it came from the user's own OTel setup.

### Required behaviour

1. **Exported marker symbol.** The module `packages/otel/src/instrument.ts` must export a constant named `providerMarker`. Its value must be the globally registered symbol `Symbol.for("inngest.otel.provider")`. Because it uses the shared symbol registry, separate copies or versions of the package resolve to the same key.

2. **Mark providers Inngest creates.** When `instrumentTracing()` finds no existing global provider, it creates and registers one. That provider object must then carry an own property keyed by `providerMarker`:
   - its value is `true`;
   - it is **non-enumerable**, so it does not appear in `Object.keys` or in spreads.

   Callers usually get the provider through `trace.getTracerProvider()`, which may return a proxy. If it does, the property must be present on the delegate the proxy wraps, which is the actual provider instance.

3. **Leave pre-existing providers alone.** Suppose a tracer provider was already registered globally before `instrumentTracing()` is called. For example, a `BasicTracerProvider` set via `trace.setGlobalTracerProvider`. That provider must **not** receive the marker, and reading `provider[providerMarker]` on it must return `undefined`.

### Example

```ts
import { trace } from "@opentelemetry/api";
import { instrumentTracing, providerMarker } from "./instrument.ts";

instrumentTracing();

let provider: any = trace.getTracerProvider();
if (typeof provider.getDelegate === "function") provider = provider.getDelegate();

provider[providerMarker]; // true if Inngest created it, undefined otherwise
```

Apart from adding the marker, the existing behaviour of `instrumentTracing()` must stay the same. That includes setting up the context manager and registering the provider.
