"""One SERVER span per dispatched request, and spec-correct error codes."""

from __future__ import annotations

import pytest
from mcp import MCPError
from mcp_types import INTERNAL_ERROR, INVALID_PARAMS
from opentelemetry.sdk.trace.export.in_memory_span_exporter import InMemorySpanExporter
from opentelemetry.trace import SpanKind, StatusCode

from fastmcp import Client, FastMCP
from fastmcp.exceptions import (
    DisabledError,
    NotFoundError,
    ToolError,
    ValidationError,
    to_mcp_error,
)
from fastmcp.server.middleware import Middleware


def _server_spans(exporter: InMemorySpanExporter):
    return [s for s in exporter.get_finished_spans() if s.kind == SpanKind.SERVER]


async def test_tool_call_has_single_rich_span(trace_exporter: InMemorySpanExporter):
    mcp = FastMCP("test-server")

    @mcp.tool()
    def greet(name: str) -> str:
        return f"Hello, {name}!"

    async with Client(mcp) as client:
        trace_exporter.clear()
        await client.call_tool("greet", {"name": "World"})

    spans = [s for s in _server_spans(trace_exporter) if s.name == "tools/call greet"]
    assert len(spans) == 1
    # One span per request: the client's implicit tools/list is its own request.
    names = [s.name for s in _server_spans(trace_exporter)]
    assert sorted(names) == ["tools/call greet", "tools/list"]
    attrs = spans[0].attributes or {}
    assert attrs["mcp.method.name"] == "tools/call"
    assert attrs["fastmcp.server.name"] == "test-server"
    assert attrs["fastmcp.component.key"] == "tool:greet@"
    assert attrs["gen_ai.tool.name"] == "greet"
    assert spans[0].parent is not None


async def test_non_component_method_gets_span(trace_exporter: InMemorySpanExporter):
    mcp = FastMCP("test-server")
    async with Client(mcp) as client:
        trace_exporter.clear()
        await client.set_logging_level("info")

    spans = _server_spans(trace_exporter)
    assert [s.name for s in spans] == ["logging/setLevel"]
    assert (spans[0].attributes or {})["fastmcp.server.name"] == "test-server"


async def test_middleware_rejection_has_error_span(
    trace_exporter: InMemorySpanExporter,
):
    class Boom(Middleware):
        async def on_call_tool(self, context, call_next):
            raise RuntimeError("boom")

    mcp = FastMCP("test-server", middleware=[Boom()])

    @mcp.tool()
    def greet() -> str:
        return "hi"

    async with Client(mcp) as client:
        trace_exporter.clear()
        with pytest.raises(Exception):
            await client.call_tool("greet", {})

    spans = _server_spans(trace_exporter)
    assert len(spans) == 1
    span = spans[0]
    assert span.status.status_code == StatusCode.ERROR
    assert (span.attributes or {})["error.type"] == "RuntimeError"
    assert any(e.name == "exception" for e in span.events)


async def test_tool_error_yields_one_span(trace_exporter: InMemorySpanExporter):
    mcp = FastMCP("test-server")

    @mcp.tool()
    def bad() -> str:
        raise ToolError("nope")

    async with Client(mcp) as client:
        trace_exporter.clear()
        await client.call_tool("bad", {}, raise_on_error=False)

    assert len(_server_spans(trace_exporter)) == 1


async def test_missing_resource_is_invalid_params():
    mcp = FastMCP("test-server")
    async with Client(mcp) as client:
        with pytest.raises(MCPError) as exc:
            await client.read_resource("file:///missing")
    assert exc.value.error.code == INVALID_PARAMS
    assert "Resource not found" in exc.value.error.message


async def test_unknown_prompt_is_invalid_params():
    mcp = FastMCP("test-server")
    async with Client(mcp) as client:
        with pytest.raises(MCPError) as exc:
            await client.get_prompt("nope")
    assert exc.value.error.code == INVALID_PARAMS
    assert "Unknown prompt" in exc.value.error.message


@pytest.mark.parametrize(
    "exc, code",
    [
        (NotFoundError("a"), INVALID_PARAMS),
        (DisabledError("a"), INVALID_PARAMS),
        (ValidationError("a"), INVALID_PARAMS),
        (ToolError("a"), INTERNAL_ERROR),
    ],
)
def test_to_mcp_error(exc: Exception, code: int):
    err = to_mcp_error(exc)
    assert err.error.code == code
    assert err.error.message == "a"


def test_to_mcp_error_passthrough_and_default():
    original = MCPError(code=-1, message="x")
    assert to_mcp_error(original) is original
    assert to_mcp_error(ToolError("y"), default_code=-5).error.code == -5
