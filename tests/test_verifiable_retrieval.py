from reasoning_engine.verifiable.retrieval import (
    MockScholarGatewayAdapter,
    ScholarGatewayAdapter,
)


def test_mock_adapter_returns_normalized_evidence():
    adapter = MockScholarGatewayAdapter()
    result = adapter.search(run_id="run_001", query="MCP prompt injection", limit=2)

    assert result.error is None
    assert len(result.evidence) == 2
    assert result.evidence[0].source_adapter == "scholar_gateway"
    assert result.evidence[0].snippet_hash.startswith("sha256:")


def test_mock_adapter_empty_query_returns_gap_error():
    adapter = MockScholarGatewayAdapter()
    result = adapter.search(run_id="run_001", query="   ", limit=2)

    assert result.evidence == []
    assert result.error is not None
    assert result.error.error_type == "unsupported_query"


def test_live_adapter_requires_token_when_live_enabled(monkeypatch):
    monkeypatch.setenv("SCHOLAR_GATEWAY_LIVE", "1")
    monkeypatch.delenv("SCHOLAR_GATEWAY_ACCESS_TOKEN", raising=False)

    adapter = ScholarGatewayAdapter()
    result = adapter.search(run_id="run_001", query="MCP", limit=1)

    assert result.evidence == []
    assert result.error is not None
    assert result.error.error_type == "auth_required"


def test_live_adapter_uses_mock_when_live_disabled(monkeypatch):
    monkeypatch.delenv("SCHOLAR_GATEWAY_LIVE", raising=False)

    adapter = ScholarGatewayAdapter()
    result = adapter.search(run_id="run_001", query="MCP", limit=1)

    assert result.error is None
    assert len(result.evidence) == 1
