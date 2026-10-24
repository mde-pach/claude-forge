# Prevent duplicate exports caused by double retrying of connection errors

## Problem

When Logfire is configured to send data to Logfire (`send_to_logfire=True`) and the backend is unreachable, the HTTP session used by the OTLP exporters (`OTLPExporterHttpSession` in `logfire._internal.exporters.otlp`) handles a failed POST like this: it retries once after a short pause. If that retry also fails, it queues the payload with its disk-backed retryer and then re-raises the original `requests` exception.

For `requests.exceptions.ConnectionError`, re-raising is harmful. The OpenTelemetry OTLP HTTP exporter already retries connection errors itself. So the same payload gets retried by OpenTelemetry *and* queued again with Logfire's retryer on each attempt. This creates two layers of retrying, and once the backend recovers the same spans, logs or metrics are exported multiple times.

The retryer's status log messages also show the problem. With a real exporter pipeline (`logfire.configure(...)`, `logfire.info(...)`, `logfire.force_flush()`), the retryer's queue grows more than it should.

## Required behaviour

- If the session's final attempt fails with a `requests.exceptions.ConnectionError`, the payload must still be handed to the retryer. This applies to both failure paths:
  - the second attempt fails;
  - the first attempt takes too long, so no in-process retry is made.
- In that case the original `ConnectionError` must **not** propagate to the OpenTelemetry exporter. Raise a new exception class instead:

  ```python
  from logfire._internal.exporters.otlp import SuppressedConnectionError  # subclass of Exception
  ```

  This exception must not be a `requests` exception, so OpenTelemetry does not treat it as retryable.
- Other `requests.exceptions.RequestException` failures keep their current behaviour: they are queued with the retryer and the original exception is re-raised.
- On Emscripten, no retry task can be added. Behaviour there is unchanged.
- The quiet wrapper exporters must treat `SuppressedConnectionError` like a request exception. They return a failure result instead of letting it propagate or log a traceback:
  - `QuietSpanExporter.export` returns `SpanExportResult.FAILURE`.
  - `QuietLogExporter.export` returns `LogRecordExportResult.FAILURE`.
  - `QuietMetricExporter.export` (in `logfire._internal.exporters.quiet_metrics`) returns `MetricExportResult.FAILURE`.

## Acceptance criteria

Setup:

- Logfire is configured with `send_to_logfire=True` and a token.
- The main session's HTTPS requests always raise `ConnectionError`.
- The retryer's session first returns retryable failures, then succeeds.

Expected results:

- Repeatedly calling `logfire.info(...)` and then `logfire.force_flush()` raises no exception in user code.
- Each failed export is queued for retry exactly once, and everything is eventually delivered.
- Every retryer warning message matches the format `Currently retrying N failed export(s) (M bytes)`.
- The first warning message reports `1` failed export.
