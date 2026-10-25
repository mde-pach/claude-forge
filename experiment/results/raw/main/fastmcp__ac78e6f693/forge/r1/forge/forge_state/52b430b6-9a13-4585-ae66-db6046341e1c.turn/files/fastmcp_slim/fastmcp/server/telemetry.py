"""Server-side telemetry helpers."""

from collections.abc import Generator
from contextlib import contextmanager

from opentelemetry import trace
from opentelemetry.context import Context
from opentelemetry.trace import Span, SpanKind, Status, StatusCode

from fastmcp.exceptions import ToolError as _ToolError
from fastmcp.telemetry import extract_trace_context, get_tracer

SEAM_SPAN_MARKER = "fastmcp.span.seam"


def record_span_exception(span: Span, exc: Exception) -> None:
    """Record an exception on a span with ERROR status, at most once per span."""
    if not span.is_recording():
        return
    attributes = getattr(span, "attributes", None) or {}
    if "error.type" in attributes:
        return
    error_type = "tool_error" if isinstance(exc, _ToolError) else type(exc).__qualname__
    span.set_attribute("error.type", error_type)
    span.record_exception(exc)
    span.set_status(Status(StatusCode.ERROR, str(exc)))


def get_auth_span_attributes() -> dict[str, str]:
    """Get auth attributes for the current request, if authenticated."""
    from fastmcp.server.dependencies import get_access_token

    attrs: dict[str, str] = {}
    try:
        token = get_access_token()
        if token:
            if token.client_id:
                attrs["enduser.id"] = token.client_id
            if token.scopes:
                attrs["enduser.scope"] = " ".join(token.scopes)
    except RuntimeError:
        pass
    return attrs


def get_session_span_attributes() -> dict[str, str]:
    """Get session attributes for the current request."""
    from fastmcp.server.dependencies import get_context

    attrs: dict[str, str] = {}
    try:
        ctx = get_context()
        if ctx.request_context is not None and ctx.session_id is not None:
            attrs["mcp.session.id"] = ctx.session_id
    except RuntimeError:
        pass
    return attrs


def _get_parent_trace_context() -> Context | None:
    """Get parent trace context from request meta for distributed tracing."""
    from fastmcp.server.dependencies import fastmcp_request_ctx

    req_ctx = fastmcp_request_ctx.get()
    if req_ctx is not None and req_ctx.meta:
        return extract_trace_context(req_ctx.meta)
    return None


@contextmanager
def server_span(
    name: str,
    method: str,
    server_name: str,
    component_type: str,
    component_key: str,
    resource_uri: str | None = None,
    tool_name: str | None = None,
    prompt_name: str | None = None,
) -> Generator[Span, None, None]:
    """Create a SERVER span with standard MCP attributes and auth context.

    Automatically records any exception on the span and sets error status.
    """
    attrs: dict[str, str] = {
        # MCP semantic conventions
        "mcp.method.name": method,
        # FastMCP-specific attributes
        "fastmcp.server.name": server_name,
        "fastmcp.component.type": component_type,
        "fastmcp.component.key": component_key,
    }
    if resource_uri is not None:
        attrs["mcp.resource.uri"] = resource_uri
    if tool_name is not None:
        attrs["gen_ai.tool.name"] = tool_name
    if prompt_name is not None:
        attrs["gen_ai.prompt.name"] = prompt_name

    current = trace.get_current_span()
    attributes = getattr(current, "attributes", None) or {}
    if current.is_recording() and attributes.get(SEAM_SPAN_MARKER):
        # The dispatcher already opened the request's SERVER span; enrich it
        # rather than emitting a second one.
        current.update_name(name)
        current.set_attributes(
            {**attrs, **get_auth_span_attributes(), **get_session_span_attributes()}
        )
        try:
            yield current
        except Exception as e:
            record_span_exception(current, e)
            raise
        return

    tracer = get_tracer()
    with tracer.start_as_current_span(
        name,
        context=_get_parent_trace_context(),
        kind=SpanKind.SERVER,
    ) as span:
        if span.is_recording():
            span.set_attributes(
                {**attrs, **get_auth_span_attributes(), **get_session_span_attributes()}
            )
        try:
            yield span
        except Exception as e:
            record_span_exception(span, e)
            raise


@contextmanager
def seam_span(method: str, server_name: str) -> Generator[Span, None, None]:
    """Create the single SERVER span for an inbound request at the dispatcher.

    Named after the method; `server_span` renames and enriches it for
    tool, resource and prompt requests. Exceptions propagating out are
    recorded on the span.
    """
    tracer = get_tracer()
    with tracer.start_as_current_span(
        method,
        context=_get_parent_trace_context(),
        kind=SpanKind.SERVER,
    ) as span:
        if span.is_recording():
            span.set_attributes(
                {
                    SEAM_SPAN_MARKER: True,
                    "mcp.method.name": method,
                    "fastmcp.server.name": server_name,
                    **get_auth_span_attributes(),
                    **get_session_span_attributes(),
                }
            )
        try:
            yield span
        except Exception as e:
            record_span_exception(span, e)
            raise


@contextmanager
def delegate_span(
    name: str,
    provider_type: str,
    component_key: str,
    method: str | None = None,
) -> Generator[Span, None, None]:
    """Create an INTERNAL span for provider delegation.

    Used by FastMCPProvider when delegating to mounted servers.
    Automatically records any exception on the span and sets error status.
    """
    tracer = get_tracer()
    with tracer.start_as_current_span(f"delegate {name}") as span:
        if span.is_recording():
            attrs: dict[str, str] = {
                "fastmcp.provider.type": provider_type,
                "fastmcp.component.key": component_key,
            }
            if method is not None:
                attrs["mcp.method.name"] = method
            span.set_attributes(attrs)
        try:
            yield span
        except Exception as e:
            record_span_exception(span, e)
            raise


__all__ = [
    "SEAM_SPAN_MARKER",
    "delegate_span",
    "get_auth_span_attributes",
    "get_session_span_attributes",
    "record_span_exception",
    "seam_span",
    "server_span",
]
