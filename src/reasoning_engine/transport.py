"""MCP transport factory and local HTTP safety checks."""

from __future__ import annotations

from typing import Any, Literal

from mcp.server.fastmcp import FastMCP

SAFE_MCP_MIN_VERSION = "1.23.0"
LOCAL_HTTP_HOSTS = {"127.0.0.1", "localhost", "::1"}
PUBLIC_HTTP_HOSTS = {"0.0.0.0", "::", ""}


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
) -> FastMCP:
    from reasoning_engine.server import register_tools

    mcp = FastMCP(
        "reasoning-engine",
        instructions="Actor-Critic-Planner-Reflexion reasoning backend for deep research",
        host=host,
        port=port,
        streamable_http_path=streamable_http_path,
        log_level=log_level,
    )
    register_tools(mcp)
    return mcp


def run_mcp(
    *,
    transport: Literal["stdio", "streamable-http"] = "stdio",
    host: str = "127.0.0.1",
    port: int = 8000,
    streamable_http_path: str = "/mcp",
    unsafe_bind_public: bool = False,
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
        )
        mcp.run(transport="streamable-http")
        return

    create_mcp().run(transport="stdio")
