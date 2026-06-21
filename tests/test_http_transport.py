import importlib.metadata

import pytest
from packaging.version import Version
from starlette.testclient import TestClient

from reasoning_engine.transport import (
    SAFE_MCP_MIN_VERSION,
    StaticBearerTokenVerifier,
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


@pytest.mark.asyncio
async def test_static_bearer_token_verifier_uses_redacted_access_token():
    verifier = StaticBearerTokenVerifier("expected-token")

    assert await verifier.verify_token("wrong-token") is None
    access = await verifier.verify_token("expected-token")

    assert access is not None
    assert access.token == "<redacted>"
    assert access.client_id == "local-http-client"


def test_streamable_http_bearer_token_required_when_configured():
    app = create_mcp(
        host="127.0.0.1",
        port=8765,
        bearer_token="expected-token",
    ).streamable_http_app()

    with TestClient(app) as client:
        missing = client.post(
            "/mcp",
            headers={
                "host": "127.0.0.1:8765",
                "origin": "http://127.0.0.1:8765",
                "accept": "application/json, text/event-stream",
            },
            json={"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}},
        )
        wrong = client.post(
            "/mcp",
            headers={
                "host": "127.0.0.1:8765",
                "origin": "http://127.0.0.1:8765",
                "accept": "application/json, text/event-stream",
                "authorization": "Bearer wrong-token",
            },
            json={"jsonrpc": "2.0", "id": 2, "method": "initialize", "params": {}},
        )
        valid = client.post(
            "/mcp",
            headers={
                "host": "127.0.0.1:8765",
                "origin": "http://127.0.0.1:8765",
                "accept": "application/json, text/event-stream",
                "authorization": "Bearer expected-token",
            },
            json={"jsonrpc": "2.0", "id": 3, "method": "initialize", "params": {}},
        )

    assert missing.status_code == 401
    assert wrong.status_code == 401
    assert valid.status_code != 401
