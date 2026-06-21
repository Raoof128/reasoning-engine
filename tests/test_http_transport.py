import importlib.metadata

import pytest
from packaging.version import Version
from starlette.testclient import TestClient

from reasoning_engine.transport import (
    SAFE_MCP_MIN_VERSION,
    create_mcp,
    validate_http_bind,
)


def test_mcp_dependency_is_dns_rebinding_safe():
    assert Version(importlib.metadata.version("mcp")) >= Version(SAFE_MCP_MIN_VERSION)


def test_validate_http_bind_defaults_to_localhost():
    config = validate_http_bind(host="127.0.0.1", port=8765)

    assert config["host"] == "127.0.0.1"
    assert config["port"] == 8765
    assert config["unsafe_bind_public"] is False


def test_validate_http_bind_rejects_public_bind_without_unsafe_flag():
    with pytest.raises(ValueError, match="unsafe-bind-public"):
        validate_http_bind(host="0.0.0.0", port=8765)


def test_validate_http_bind_allows_public_bind_with_unsafe_flag():
    config = validate_http_bind(host="0.0.0.0", port=8765, unsafe_bind_public=True)

    assert config["host"] == "0.0.0.0"
    assert config["unsafe_bind_public"] is True


def test_http_and_stdio_tool_registry_match():
    stdio_tools = {tool.name for tool in create_mcp()._tool_manager.list_tools()}
    http_tools = {
        tool.name for tool in create_mcp(host="127.0.0.1", port=8765)._tool_manager.list_tools()
    }

    assert http_tools == stdio_tools
    assert "run_research_pipeline_tool" in http_tools


def test_streamable_http_localhost_host_origin_allowed():
    app = create_mcp(host="127.0.0.1", port=8765).streamable_http_app()
    with TestClient(app) as client:
        response = client.post(
            "/mcp",
            headers={
                "host": "127.0.0.1:8765",
                "origin": "http://127.0.0.1:8765",
                "accept": "application/json, text/event-stream",
            },
            json={"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}},
        )

    assert response.status_code != 403


def test_streamable_http_suspicious_host_origin_rejected():
    app = create_mcp(host="127.0.0.1", port=8765).streamable_http_app()
    with TestClient(app) as client:
        response = client.post(
            "/mcp",
            headers={
                "host": "evil.example",
                "origin": "https://evil.example",
                "accept": "application/json, text/event-stream",
            },
            json={"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}},
        )

    assert response.status_code in {403, 421}
