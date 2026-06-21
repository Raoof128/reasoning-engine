"""MCP transport factory and local HTTP safety checks."""

from __future__ import annotations

import hmac
from typing import Any, Literal

from mcp.server.auth.provider import AccessToken, TokenVerifier
from mcp.server.auth.settings import AuthSettings
from mcp.server.fastmcp import FastMCP

SAFE_MCP_MIN_VERSION = "1.23.0"
LOCAL_HTTP_HOSTS = {"127.0.0.1", "localhost", "::1"}
PUBLIC_HTTP_HOSTS = {"0.0.0.0", "::", ""}


class StaticBearerTokenVerifier(TokenVerifier):
    """Validate a single local bearer token for laptop HTTP MCP mode."""

    def __init__(self, token: str) -> None:
        if not token:
            raise ValueError("bearer token must not be empty")
        self._token = token

    async def verify_token(self, token: str) -> AccessToken | None:
        if not hmac.compare_digest(token, self._token):
            return None
        return AccessToken(
            token="<redacted>",
            client_id="local-http-client",
            scopes=["mcp:access"],
        )


def validate_http_bind(
    *,
    host: str,
    port: int,
    unsafe_bind_public: bool = False,
) -> dict[str, Any]:
    normalized_host = host.strip()
    if not 1 <= int(port) <= 65535:
        raise ValueError("port must be between 1 and 65535")
    if normalized_host in PUBLIC_HTTP_HOSTS and not unsafe_bind_public:
        raise ValueError("public HTTP bind requires --unsafe-bind-public")
    return {
        "host": normalized_host,
        "port": int(port),
        "unsafe_bind_public": unsafe_bind_public,
    }


def create_mcp(
    *,
    host: str = "127.0.0.1",
    port: int = 8000,
    streamable_http_path: str = "/mcp",
    log_level: Literal["DEBUG", "INFO", "WARNING", "ERROR", "CRITICAL"] = "INFO",
    bearer_token: str | None = None,
) -> FastMCP:
    from reasoning_engine.server import register_tools

    auth = (
        AuthSettings(
            issuer_url=f"http://{_url_host(host)}:{port}",
            required_scopes=[],
            resource_server_url=None,
        )
        if bearer_token
        else None
    )
    mcp = FastMCP(
        "reasoning-engine",
        instructions="Actor-Critic-Planner-Reflexion reasoning backend for deep research",
        host=host,
        port=port,
        streamable_http_path=streamable_http_path,
        log_level=log_level,
        auth=auth,
        token_verifier=StaticBearerTokenVerifier(bearer_token) if bearer_token else None,
    )
    register_tools(mcp)
    return mcp


def _url_host(host: str) -> str:
    if host == "::1":
        return "[::1]"
    if host in PUBLIC_HTTP_HOSTS:
        return "localhost"
    return host


def run_mcp(
    *,
    transport: Literal["stdio", "streamable-http"] = "stdio",
    host: str = "127.0.0.1",
    port: int = 8000,
    streamable_http_path: str = "/mcp",
    unsafe_bind_public: bool = False,
    bearer_token: str | None = None,
) -> None:
    if transport == "streamable-http":
        bind = validate_http_bind(
            host=host,
            port=port,
            unsafe_bind_public=unsafe_bind_public,
        )
        mcp = create_mcp(
            host=bind["host"],
            port=bind["port"],
            streamable_http_path=streamable_http_path,
            bearer_token=bearer_token,
        )
        mcp.run(transport="streamable-http")
        return

    create_mcp().run(transport="stdio")
