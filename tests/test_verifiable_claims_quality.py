from reasoning_engine.verifiable.claims import extract_claims, verify_claims
from reasoning_engine.verifiable.models import EvidenceRecord, utc_now
from reasoning_engine.verifiable.quality import run_quality_gate


def _evidence(snippet: str) -> EvidenceRecord:
    return EvidenceRecord(
        evidence_id="ev_0001",
        run_id="run_001",
        source_adapter="scholar_gateway",
        source_type="peer_reviewed_article",
        title="Evidence",
        authors=["A. Author"],
        year=2026,
        publisher="Wiley",
        venue="Journal",
        doi="10.1000/example",
        url="https://doi.org/10.1000/example",
        retrieved_at=utc_now(),
        query="query",
        rank=1,
        score=0.9,
        snippet=snippet,
        licence_notes=None,
        risk_flags=[],
        metadata={},
    )


def test_extract_claims_splits_sentences_and_marks_quantitative():
    claims = extract_claims(
        run_id="run_001",
        text="Scholar Gateway has 8 million articles. This is a good direction.",
        domain="general",
    )

    assert len(claims) == 2
    assert claims[0].claim_type == "quantitative"
    assert claims[0].requires_citation is True


def test_verify_claims_supports_claim_when_evidence_contains_terms():
    claims = extract_claims("run_001", "Scholar Gateway exposes semantic search.", "general")
    verifications = verify_claims(
        claims,
        [_evidence("Scholar Gateway exposes semantic search for Wiley articles.")],
    )

    assert verifications[0].support_status == "supported"
    assert verifications[0].evidence_ids == ["ev_0001"]


def test_verify_claims_marks_missing_evidence():
    claims = extract_claims("run_001", "Scholar Gateway exposes semantic search.", "general")
    verifications = verify_claims(claims, [])

    assert verifications[0].support_status == "needs_more_evidence"
    assert verifications[0].missing_evidence


def test_quality_gate_blocks_unsupported_final_claims():
    claims = extract_claims("run_001", "Scholar Gateway exposes semantic search.", "general")
    verifications = verify_claims(claims, [])
    gate = run_quality_gate(run_id="run_001", claims=claims, verifications=verifications, gaps=[])

    assert gate["result"] == "blocked"
    assert "unsupported factual claims remain" in gate["blocking_failures"]


def test_quality_gate_passes_supported_claims():
    claims = extract_claims("run_001", "Scholar Gateway exposes semantic search.", "general")
    verifications = verify_claims(
        claims,
        [_evidence("Scholar Gateway exposes semantic search for Wiley articles.")],
    )
    gate = run_quality_gate(run_id="run_001", claims=claims, verifications=verifications, gaps=[])

    assert gate["result"] == "pass"
    assert gate["blocking_failures"] == []
