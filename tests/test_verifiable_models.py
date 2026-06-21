from reasoning_engine.verifiable.models import (
    ClaimRecord,
    EvidenceRecord,
    ResearchRun,
    RetrievalError,
    normalize_status,
)


def test_evidence_record_hashes_snippet():
    evidence = EvidenceRecord(
        evidence_id="ev_0001",
        run_id="run_001",
        source_adapter="scholar_gateway",
        source_type="peer_reviewed_article",
        title="A useful paper",
        authors=["A. Researcher"],
        year=2026,
        publisher="Wiley",
        venue="Journal",
        doi="10.1000/example",
        url="https://doi.org/10.1000/example",
        retrieved_at="2026-06-21T00:00:00Z",
        query="test query",
        rank=1,
        score=0.82,
        snippet="Important evidence.",
        licence_notes=None,
        risk_flags=[],
        metadata={},
    )

    payload = evidence.to_dict()

    assert payload["snippet_hash"].startswith("sha256:")
    assert payload["snippet"] == "Important evidence."


def test_claim_record_defaults_to_needs_more_evidence():
    claim = ClaimRecord(
        claim_id="claim_0001",
        run_id="run_001",
        text="Scholar Gateway returns Wiley article metadata.",
        claim_type="empirical",
        domain="general",
        importance="medium",
        risk_level="low",
        requires_citation=True,
        created_from="draft",
    )

    assert claim.status == "needs_more_evidence"
    assert claim.evidence_ids == []


def test_retrieval_error_serializes_without_token_values():
    error = RetrievalError(
        error_type="auth_required",
        message="Set SCHOLAR_GATEWAY_ACCESS_TOKEN",
        retryable=False,
        metadata={"env_var": "SCHOLAR_GATEWAY_ACCESS_TOKEN"},
    )

    assert error.to_dict() == {
        "error_type": "auth_required",
        "message": "Set SCHOLAR_GATEWAY_ACCESS_TOKEN",
        "retryable": False,
        "metadata": {"env_var": "SCHOLAR_GATEWAY_ACCESS_TOKEN"},
    }


def test_research_run_has_profile_and_mode():
    run = ResearchRun.create(query="Explain MCP security", profile="security", mode="standard")

    assert run.run_id.startswith("run_")
    assert run.query == "Explain MCP security"
    assert run.profile == "security"
    assert run.mode == "standard"


def test_normalize_status_rejects_unknown_status():
    assert normalize_status("supported") == "supported"

    try:
        normalize_status("unknown")
    except ValueError as exc:
        assert "support status" in str(exc)
    else:
        raise AssertionError("unknown status should fail")
