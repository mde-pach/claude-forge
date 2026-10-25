You are working in the git repository in the current directory. Make the change described below. Work autonomously: nobody will answer questions.

# Extract AI call latency as AI metadata

## Problem

The OTel AI metadata extractor in `packages/inngest/src/components/execution/otel/aiExtractor.ts` only reads `gen_ai.*` span attributes. It cannot capture data that comes from the span itself, such as timing. As a result, AI step metadata has no latency information.

## Requirements

### Span-based extraction

- Remove the exported `extractAIMetadataFromAttributes(attributes)`.
- Add `extractAIMetadataFromSpan(span: ReadableSpan): AIMetadata`, exported from the same module. `ReadableSpan` is from `@opentelemetry/sdk-trace-base`.
- The function must work with span-like objects that provide only `attributes`, `startTime` and `endTime`.
- All existing attribute behaviour stays the same:
  - allowlisting of `gen_ai.*` keys;
  - the deprecated `gen_ai.system` fallback for `provider`;
  - numeric coercion of quoted strings;
  - dropping empty text and empty lists;
  - ignoring content and sensitive keys.
- `InngestMetadataSpanProcessor` must use span-based extraction. Metadata it pushes for AI spans therefore includes `latencyMs`.

### `latencyMs`

- Add an optional `latencyMs?: number` field to `AIMetadata`.
- The value is the span's duration in milliseconds, computed from its OTel `HrTime` start and end times (`[seconds, nanoseconds]`):
  - Convert each time to milliseconds as `seconds * 1000 + nanoseconds / 1e6`.
  - Subtract the start value from the end value.
  - For example, a start of `[10, 0]` and an end of `[10, 500_000_000]` gives `500`. A zero-length span gives `0`.
- Include the value only when it is a finite number.
- `latencyMs` is an *optional* field: it is populated only if at least one other recognised field produced a value for the same span.
  - A span with no recognised `gen_ai.*` attributes must still yield `{}`.
  - A span whose recognised attributes are all dropped (for example, an empty `gen_ai.request.model`) must also yield `{}`.

### Aggregation and serialisation

- `aggregate(a, b)` must **sum** `latencyMs` across AI calls in the same step. For example, `100` and `50` give `150`.
- `toInngestAIMetadataValues` must emit `latencyMs` under the snake_case key `latency_ms`.

### `FIELD_SPECS`

- Each entry in the exported `FIELD_SPECS` must still expose `field`, the canonical `AIMetadata` field name. Each field must still be mapped at most once.
- Entries may carry an `optional?: boolean` flag. `latencyMs` must be the only entry with `optional` truthy.
- Entries no longer need to be tied to a `gen_ai.*` source key, so that span-derived fields can be expressed.
