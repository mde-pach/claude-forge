from __future__ import annotations

import ipaddress
from collections.abc import AsyncGenerator, Callable, Generator, Sequence
from contextlib import asynccontextmanager, contextmanager
from contextvars import ContextVar
from fnmatch import fnmatchcase
from typing import TYPE_CHECKING, Any
from urllib.parse import urlsplit
from uuid import uuid4

from mcp.server.auth.routes import build_resource_metadata_url
from mcp.server.lowlevel.server import LifespanResultT
from mcp.server.sse import SseServerTransport
from mcp.server.streamable_http import (
    EventStore,
)
from mcp.server.streamable_http_manager import StreamableHTTPSessionManager
from mcp.server.transport_security import TransportSecuritySettings
from starlette.applications import Starlette
from starlette.datastructures import Headers
from starlette.middleware import Middleware
from starlette.requests import Request
from starlette.responses import PlainTextResponse, Response
from starlette.routing import BaseRoute, Mount, Route
from starlette.types import ASGIApp, Lifespan, Receive, Scope, Send

from fastmcp.server.auth import AuthProvider
from fastmcp.server.auth.middleware import RequireAuthMiddleware
from fastmcp.server.event_store import SessionScopedEventStore
from fastmcp.utilities.logging import get_logger

if TYPE_CHECKING:
    from fastmcp.server.server import FastMCP

logger = get_logger(__name__)


DEFAULT_ALLOWED_HOSTS = ("127.0.0.1", "localhost", "::1")
_DEFAULT_PORTS = {"http": 80, "https": 443, "ws": 80, "wss": 443}


def _split_host_port(value: str) -> tuple[str, int | None]:
    """Split a Host header value into a lowercase host and optional port."""
    value = value.strip().lower()
    if value.startswith("["):
        host, _, rest = value[1:].partition("]")
        port_str = rest.removeprefix(":")
    elif value.count(":") == 1:
        host, _, port_str = value.partition(":")
    else:
        host, port_str = value, ""
    return host, int(port_str) if port_str.isdigit() else None


def _is_loopback(host: str) -> bool:
    if host in DEFAULT_ALLOWED_HOSTS:
        return True
    try:
        return ipaddress.ip_address(host).is_loopback
    except ValueError:
        return False


def _matches(value: str, patterns: Sequence[str]) -> bool:
    value = value.lower()
    return any(fnmatchcase(value, pattern.lower()) for pattern in patterns)


def _normalize_origin(
    scheme: str, host: str, port: int | None
) -> tuple[str, str, int | None]:
    return scheme.lower(), host.lower(), port or _DEFAULT_PORTS.get(scheme.lower())


class HostOriginProtectionMiddleware:
    """Reject HTTP requests with untrusted Host or Origin headers.

    Protects against DNS rebinding. Responds 421 for an untrusted Host and 403
    for an untrusted Origin. Non-HTTP scopes pass through. This does not
    configure CORS.
    """

    def __init__(
        self,
        app: ASGIApp,
        allowed_hosts: Sequence[str] | None = None,
        allowed_origins: Sequence[str] | None = None,
    ) -> None:
        self.app = app
        self.allowed_hosts = [*DEFAULT_ALLOWED_HOSTS, *(allowed_hosts or [])]
        self.allowed_origins = list(allowed_origins or [])

    async def __call__(self, scope: Scope, receive: Receive, send: Send) -> None:
        if scope["type"] != "http":
            await self.app(scope, receive, send)
            return

        headers = Headers(scope=scope)
        host_header = headers.get("host", "")
        host, host_port = _split_host_port(host_header)
        if not host or not _matches(host, self.allowed_hosts):
            await PlainTextResponse("Misdirected Request", status_code=421)(
                scope, receive, send
            )
            return

        origin = headers.get("origin")
        if origin is not None and not self._origin_allowed(
            origin, host, host_port, scope.get("scheme", "http")
        ):
            await PlainTextResponse("Forbidden Origin", status_code=403)(
                scope, receive, send
            )
            return

        await self.app(scope, receive, send)

    def _origin_allowed(
        self, origin: str, host: str, host_port: int | None, scheme: str
    ) -> bool:
        if _matches(origin, self.allowed_origins):
            return True
        try:
            parsed = urlsplit(origin)
            origin_host = parsed.hostname
            origin_port = parsed.port
        except ValueError:
            return False
        if not parsed.scheme or not origin_host:
            return False
        if _is_loopback(origin_host) and _is_loopback(host):
            return True
        return _normalize_origin(
            parsed.scheme, origin_host, origin_port
        ) == _normalize_origin(scheme, host, host_port)


class FastMCPStreamableHTTPSessionManager(StreamableHTTPSessionManager):
    """Session manager that scopes resumability storage per transport session."""

    def __init__(
        self,
        app: Any,
        event_store: EventStore | None = None,
        json_response: bool = False,
        stateless: bool = False,
        security_settings: TransportSecuritySettings | None = None,
        retry_interval: int | None = None,
    ) -> None:
        self._shared_event_store: EventStore | None = None
        super().__init__(
            app=app,
            event_store=event_store,
            json_response=json_response,
            stateless=stateless,
            security_settings=security_settings,
            retry_interval=retry_interval,
        )

    @property
    def event_store(self) -> EventStore | None:
        if self._shared_event_store is None:
            return None
        # The SDK reads `self.event_store` once when constructing each transport.
        # A fresh adapter gives that transport a private stream namespace.
        return SessionScopedEventStore(self._shared_event_store, session_id=uuid4().hex)

    @event_store.setter
    def event_store(self, event_store: EventStore | None) -> None:
        self._shared_event_store = event_store


class StreamableHTTPASGIApp:
    """ASGI application wrapper for Streamable HTTP server transport."""

    def __init__(self, session_manager: StreamableHTTPSessionManager | None):
        self.session_manager = session_manager

    async def __call__(self, scope: Scope, receive: Receive, send: Send) -> None:
        try:
            if self.session_manager is None:
                raise RuntimeError(
                    "Task group is not initialized. Make sure to use run()."
                )
            await self.session_manager.handle_request(scope, receive, send)
        except RuntimeError as e:
            if str(e) == "Task group is not initialized. Make sure to use run().":
                logger.error(
                    f"Original RuntimeError from mcp library: {e}", exc_info=True
                )
                new_error_message = (
                    "FastMCP's StreamableHTTPSessionManager task group was not initialized. "
                    "This commonly occurs when the FastMCP application's lifespan is not "
                    "passed to the parent ASGI application (e.g., FastAPI or Starlette). "
                    "Please ensure you are setting `lifespan=mcp_app.lifespan` in your "
                    "parent app's constructor, where `mcp_app` is the application instance "
                    "returned by `fastmcp_instance.http_app()`. \\n"
                    "For more details, see the FastMCP ASGI integration documentation: "
                    "https://gofastmcp.com/deployment/asgi"
                )
                # Raise a new RuntimeError that includes the original error's message
                # for full context, but leads with the more helpful guidance.
                raise RuntimeError(f"{new_error_message}\\nOriginal error: {e}") from e
            else:
                # Re-raise other RuntimeErrors if they don't match the specific message
                raise


_current_http_request: ContextVar[Request | None] = ContextVar(
    "http_request",
    default=None,
)


class StarletteWithLifespan(Starlette):
    @property
    def lifespan(self) -> Lifespan[Starlette]:
        return self.router.lifespan_context


@contextmanager
def set_http_request(request: Request) -> Generator[Request, None, None]:
    token = _current_http_request.set(request)
    try:
        yield request
    finally:
        _current_http_request.reset(token)


class RequestContextMiddleware:
    """
    Middleware that stores each request in a ContextVar and sets transport type.
    """

    def __init__(self, app):
        self.app = app

    async def __call__(self, scope, receive, send):
        if scope["type"] == "http":
            from fastmcp.server.context import reset_transport, set_transport

            # Get transport type from app state (set during app creation)
            transport_type = getattr(scope["app"].state, "transport_type", None)
            transport_token = set_transport(transport_type) if transport_type else None
            try:
                with set_http_request(Request(scope)):
                    await self.app(scope, receive, send)
            finally:
                if transport_token is not None:
                    reset_transport(transport_token)
        else:
            await self.app(scope, receive, send)


def create_base_app(
    routes: list[BaseRoute],
    middleware: list[Middleware],
    debug: bool = False,
    lifespan: Callable | None = None,
) -> StarletteWithLifespan:
    """Create a base Starlette app with common middleware and routes.

    Args:
        routes: List of routes to include in the app
        middleware: List of middleware to include in the app
        debug: Whether to enable debug mode
        lifespan: Optional lifespan manager for the app

    Returns:
        A Starlette application
    """
    # Always add RequestContextMiddleware as the outermost middleware
    middleware.insert(0, Middleware(RequestContextMiddleware))  # type: ignore[arg-type]

    return StarletteWithLifespan(
        routes=routes,
        middleware=middleware,
        debug=debug,
        lifespan=lifespan,
    )


def create_sse_app(
    server: FastMCP[LifespanResultT],
    message_path: str,
    sse_path: str,
    auth: AuthProvider | None = None,
    debug: bool = False,
    routes: list[BaseRoute] | None = None,
    middleware: list[Middleware] | None = None,
) -> StarletteWithLifespan:
    """Return an instance of the SSE server app.

    Args:
        server: The FastMCP server instance
        message_path: Path for SSE messages
        sse_path: Path for SSE connections
        auth: Optional authentication provider (AuthProvider)
        debug: Whether to enable debug mode
        routes: Optional list of custom routes
        middleware: Optional list of middleware
    Returns:
        A Starlette application with RequestContextMiddleware
    """

    server_routes: list[BaseRoute] = []
    server_middleware: list[Middleware] = []

    # Set up SSE transport
    sse = SseServerTransport(message_path)

    # Create handler for SSE connections
    async def handle_sse(scope: Scope, receive: Receive, send: Send) -> Response:
        async with sse.connect_sse(scope, receive, send) as streams:
            await server._mcp_server.run(
                streams[0],
                streams[1],
                server._mcp_server.create_initialization_options(),
            )
        return Response()

    # Set up auth if enabled
    if auth:
        # Get auth middleware from the provider
        auth_middleware = auth.get_middleware()

        # Get auth provider's own routes (OAuth endpoints, metadata, etc)
        auth_routes = auth.get_routes(mcp_path=sse_path)
        server_routes.extend(auth_routes)
        server_middleware.extend(auth_middleware)

        # Build RFC 9728-compliant metadata URL
        resource_url = auth._get_resource_url(sse_path)
        resource_metadata_url = (
            build_resource_metadata_url(resource_url) if resource_url else None
        )

        # Create protected SSE endpoint route
        server_routes.append(
            Route(
                sse_path,
                endpoint=RequireAuthMiddleware(
                    handle_sse,
                    auth.required_scopes,
                    resource_metadata_url,
                ),
                methods=["GET"],
            )
        )

        # Wrap the SSE message endpoint with RequireAuthMiddleware
        server_routes.append(
            Mount(
                message_path,
                app=RequireAuthMiddleware(
                    sse.handle_post_message,
                    auth.required_scopes,
                    resource_metadata_url,
                ),
            )
        )
    else:
        # No auth required
        async def sse_endpoint(request: Request) -> Response:
            return await handle_sse(request.scope, request.receive, request._send)

        server_routes.append(
            Route(
                sse_path,
                endpoint=sse_endpoint,
                methods=["GET"],
            )
        )
        server_routes.append(
            Mount(
                message_path,
                app=sse.handle_post_message,
            )
        )

    # Add custom routes with lowest precedence
    if routes:
        server_routes.extend(routes)
    server_routes.extend(server._get_additional_http_routes())

    # Add middleware
    if middleware:
        server_middleware.extend(middleware)

    @asynccontextmanager
    async def lifespan(app: Starlette) -> AsyncGenerator[None, None]:
        async with server._lifespan_manager():
            yield

    # Create and return the app
    app = create_base_app(
        routes=server_routes,
        middleware=server_middleware,
        debug=debug,
        lifespan=lifespan,
    )
    # Store the FastMCP server instance on the Starlette app state
    app.state.fastmcp_server = server
    app.state.path = sse_path
    app.state.transport_type = "sse"

    return app


def create_streamable_http_app(
    server: FastMCP[LifespanResultT],
    streamable_http_path: str,
    event_store: EventStore | None = None,
    retry_interval: int | None = None,
    auth: AuthProvider | None = None,
    json_response: bool = False,
    stateless_http: bool = False,
    debug: bool = False,
    routes: list[BaseRoute] | None = None,
    middleware: list[Middleware] | None = None,
    host_origin_protection: bool = True,
    allowed_hosts: Sequence[str] | None = None,
    allowed_origins: Sequence[str] | None = None,
) -> StarletteWithLifespan:
    """Return an instance of the StreamableHTTP server app.

    Args:
        server: The FastMCP server instance
        streamable_http_path: Path for StreamableHTTP connections
        event_store: Optional event store for SSE polling/resumability
        retry_interval: Optional retry interval in milliseconds for SSE polling.
            Controls how quickly clients should reconnect after server-initiated
            disconnections. Requires event_store to be set. Defaults to SDK default.
        auth: Optional authentication provider (AuthProvider)
        json_response: Whether to use JSON response format
        stateless_http: Whether to use stateless mode (new transport per request)
        debug: Whether to enable debug mode
        routes: Optional list of custom routes
        middleware: Optional list of middleware
        host_origin_protection: Reject requests with untrusted Host or Origin
            headers (DNS rebinding protection). Does not configure CORS.
        allowed_hosts: Extra Host values to accept (wildcards supported) beyond
            127.0.0.1, localhost and ::1
        allowed_origins: Origins to accept (wildcards supported)

    Returns:
        A Starlette application with StreamableHTTP support
    """
    server_routes: list[BaseRoute] = []
    server_middleware: list[Middleware] = []

    if host_origin_protection:
        server_middleware.append(
            Middleware(
                HostOriginProtectionMiddleware,
                allowed_hosts=allowed_hosts,
                allowed_origins=allowed_origins,
            )
        )

    # Create the ASGI app wrapper (session manager is set each lifespan cycle)
    streamable_http_app = StreamableHTTPASGIApp(None)

    # Add StreamableHTTP routes with or without auth
    if auth:
        # Get auth middleware from the provider
        auth_middleware = auth.get_middleware()

        # Get auth provider's own routes (OAuth endpoints, metadata, etc)
        auth_routes = auth.get_routes(mcp_path=streamable_http_path)
        server_routes.extend(auth_routes)
        server_middleware.extend(auth_middleware)

        # Build RFC 9728-compliant metadata URL
        resource_url = auth._get_resource_url(streamable_http_path)
        resource_metadata_url = (
            build_resource_metadata_url(resource_url) if resource_url else None
        )

        # Create protected HTTP endpoint route
        # Stateless servers have no session tracking, so GET SSE streams
        # (for server-initiated notifications) serve no purpose.
        http_methods = (
            ["POST", "DELETE"] if stateless_http else ["GET", "POST", "DELETE"]
        )
        server_routes.append(
            Route(
                streamable_http_path,
                endpoint=RequireAuthMiddleware(
                    streamable_http_app,
                    auth.required_scopes,
                    resource_metadata_url,
                ),
                methods=http_methods,
            )
        )
    else:
        # No auth required
        http_methods = ["POST", "DELETE"] if stateless_http else None
        server_routes.append(
            Route(
                streamable_http_path,
                endpoint=streamable_http_app,
                methods=http_methods,
            )
        )

    # Add custom routes with lowest precedence
    if routes:
        server_routes.extend(routes)
    server_routes.extend(server._get_additional_http_routes())

    # Add middleware
    if middleware:
        server_middleware.extend(middleware)

    # Create a lifespan manager to start and stop the session manager
    @asynccontextmanager
    async def lifespan(app: Starlette) -> AsyncGenerator[None, None]:
        streamable_http_app.session_manager = FastMCPStreamableHTTPSessionManager(
            app=server._mcp_server,
            event_store=event_store,
            retry_interval=retry_interval,
            json_response=json_response,
            stateless=stateless_http,
        )
        async with (
            server._lifespan_manager(),
            streamable_http_app.session_manager.run(),
        ):
            try:
                yield
            finally:
                # Gracefully terminate active streamable-HTTP transports before
                # the session manager's task group is cancelled. Without this,
                # active SSE/streaming responses are aborted mid-flight and
                # Uvicorn logs "ASGI callable returned without completing
                # response." See PrefectHQ/fastmcp#3025.
                sm = streamable_http_app.session_manager
                # `_server_instances` is a private attribute of the upstream
                # `StreamableHTTPSessionManager` (mcp SDK); termination is
                # idempotent and tolerates new instances being added concurrently.
                for transport in list(sm._server_instances.values()):
                    try:
                        await transport.terminate()
                    except Exception:
                        logger.debug(
                            "Error terminating streamable-HTTP transport on shutdown",
                            exc_info=True,
                        )

    # Create and return the app with lifespan
    app = create_base_app(
        routes=server_routes,
        middleware=server_middleware,
        debug=debug,
        lifespan=lifespan,
    )
    # Store the FastMCP server instance on the Starlette app state
    app.state.fastmcp_server = server
    app.state.path = streamable_http_path
    app.state.transport_type = "streamable-http"

    return app
