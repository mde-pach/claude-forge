import pytest
from starlette.applications import Starlette
from starlette.responses import PlainTextResponse
from starlette.routing import Route
from starlette.testclient import TestClient

from fastmcp import FastMCP
from fastmcp.server.http import (
    HostOriginProtectionMiddleware,
    create_streamable_http_app,
)


def _status(
    host: str,
    origin: str | None = None,
    *,
    scheme: str = "http",
    allowed_hosts: list[str] | None = None,
    allowed_origins: list[str] | None = None,
) -> int:
    async def ok(request):
        return PlainTextResponse("ok")

    app = Starlette(routes=[Route("/", ok)])
    app.add_middleware(
        HostOriginProtectionMiddleware,
        allowed_hosts=allowed_hosts,
        allowed_origins=allowed_origins,
    )
    headers = {"host": host}
    if origin is not None:
        headers["origin"] = origin
    client = TestClient(app, base_url=f"{scheme}://{host}")
    return client.get("/", headers=headers).status_code


@pytest.mark.parametrize(
    "host", ["localhost", "LOCALHOST:8000", "127.0.0.1:80", "[::1]:8000"]
)
def test_default_hosts_allowed(host):
    assert _status(host) == 200


def test_untrusted_host_rejected():
    assert _status("evil.example") == 421


def test_allowed_hosts_wildcards():
    assert _status("a.example.com", allowed_hosts=["*.example.com"]) == 200
    assert _status("evil.com", allowed_hosts=["*"]) == 200


def test_loopback_origin_with_loopback_host():
    assert _status("127.0.0.1", "http://localhost:3000") == 200


def test_loopback_origin_rejected_for_remote_host():
    assert (
        _status(
            "mcp.example.com",
            "http://localhost:3000",
            allowed_hosts=["mcp.example.com"],
        )
        == 403
    )
    assert (
        _status(
            "mcp.example.com",
            "http://localhost:3000",
            allowed_hosts=["mcp.example.com"],
            allowed_origins=["http://localhost:3000"],
        )
        == 200
    )


def test_untrusted_origin_rejected():
    assert _status("localhost", "http://evil.example") == 403


def test_same_origin_normalizes_default_ports():
    kwargs = {"scheme": "https", "allowed_hosts": ["mcp.example.com"]}
    assert _status("mcp.example.com", "https://mcp.example.com:443", **kwargs) == 200
    assert _status("mcp.example.com", "http://mcp.example.com", **kwargs) == 403
    assert _status("mcp.example.com", "https://mcp.example.com:8443", **kwargs) == 403


def test_streamable_http_app_rejects_before_session_created():
    app = create_streamable_http_app(FastMCP("x"), "/mcp")
    client = TestClient(app, base_url="http://evil.example")
    response = client.post("/mcp", json={})
    assert response.status_code == 421
    assert response.text == "Misdirected Request"
    assert "mcp-session-id" not in response.headers


def test_protection_can_be_disabled():
    app = create_streamable_http_app(FastMCP("x"), "/mcp", host_origin_protection=False)
    assert not any(m.cls is HostOriginProtectionMiddleware for m in app.user_middleware)
