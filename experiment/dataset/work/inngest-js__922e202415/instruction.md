# `step.run` output type collapses to `{}` when `Jsonify` is applied more than once

## Problem

The `Jsonify<T>` type is exported from `packages/inngest/src/helpers/jsonify.ts`. It describes how a value looks after JSON serialisation. It breaks when it is applied to its own output and the object contains an optional property.

For example, given:

```ts
type Widget = { media: { mediaId: string; label?: string }[] };
```

- `Jsonify<Widget>` looks correct. Accessing `["media"][number]["mediaId"]` yields `string`.
- `Jsonify<Jsonify<Widget>>` silently degrades. The nested object collapses to `{}`, or to an error/`any`-like type, instead of keeping its shape.

Real usage hits this in two places:

1. **Multiple middleware.** Each middleware's default step output transform applies `Jsonify`. A client configured with two or more middleware therefore applies it repeatedly. For example, with `new Inngest({ id, middleware: [MiddlewareA, MiddlewareB] })`, where both classes extend `Middleware.BaseMiddleware` and set only a readonly `id`, `await step.run("load", async (): Promise<Widget> => ...)` loses the shape of `media`.
2. **`step.invoke`.** A child function's handler might return a `step.run(...)` result. That return type is already jsonified, and `step.invoke` applies `Jsonify` again. The parent then gets a broken type for the invoked result.

## Requirements

- Re-applying `Jsonify` must be a no-op. `Jsonify<Jsonify<T>>` must be equivalent to `Jsonify<T>`, and every property must stay accessible with the correct type. This includes:
  - plain object types with optional properties, including when nested in arrays;
  - interfaces that combine a string index signature (e.g. `[x: string]: any`) with known keys whose values contain optional properties. For example, with `items: { id: string; label?: string }[]`, the type `Jsonify<Jsonify<Foo>>["items"][number]["id"]` must be `string`.
- With multiple middleware registered, `step.run` returning `Promise<Widget>` must give a result where:
  - `widget.media` is not `any`;
  - `widget.media` is exactly `{ mediaId: string; label?: string }[]`;
  - `widget.media[0]!.mediaId` is `string`.
- Consider a child function whose handler returns `step.run("load", async (): Promise<Widget> => ...)` directly. When a parent calls `await step.invoke("invoke-child", { function: childFn })`, the result's `media` must not be `any`, and `media[0]!.mediaId` must be `string`.
- Existing `Jsonify` behaviour must not change. That includes preserving `unknown`, handling mixed index-signature objects, and the behaviour covered by the existing type tests.
- The public name and import path of `Jsonify` stay the same.
- Add a changeset for the `inngest` package that describes the bug fix.
