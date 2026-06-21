from pathlib import Path

from reasoning_engine.verifiable.models import ClaimRecord, EvidenceRecord, ResearchRun, utc_now
from reasoning_engine.verifiable.runpack import export_run_pack, verify_run_pack_attestation


def _evidence(run_id: str) -> EvidenceRecord:
    return EvidenceRecord(
        evidence_id="ev_0001",
        run_id=run_id,
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
        snippet="Scholar Gateway exposes semantic search.",
        licence_notes=None,
        risk_flags=[],
        metadata={},
    )


def test_export_run_pack_writes_required_artifacts(tmp_path):
    run = ResearchRun.create("Explain Scholar Gateway", "general", "standard")
    claim = ClaimRecord(
        claim_id="claim_0001",
        run_id=run.run_id,
        text="Scholar Gateway exposes semantic search",
        claim_type="empirical",
        domain="general",
        importance="high",
        risk_level="low",
        requires_citation=True,
        created_from="test",
        status="supported",
        evidence_ids=["ev_0001"],
    )

    output = export_run_pack(
        base_dir=tmp_path,
        run=run,
        evidence=[_evidence(run.run_id).to_dict()],
        gaps=[],
        claims=[claim.to_dict()],
        verifications=[],
        provenance=[],
        quality_gate={"result": "pass", "blocking_failures": [], "warnings": []},
        report_markdown="# Report\n\nSupported report.",
    )

    assert (output / "run.json").exists()
    assert (output / "evidence_ledger.json").exists()
    assert (output / "claims.json").exists()
    assert (output / "attestation.json").exists()
    assert verify_run_pack_attestation(output)["valid"] is True


def test_tampered_run_pack_fails_verification(tmp_path):
    run = ResearchRun.create("Explain Scholar Gateway", "general", "standard")
    output = export_run_pack(
        base_dir=tmp_path,
        run=run,
        evidence=[],
        gaps=[],
        claims=[],
        verifications=[],
        provenance=[],
        quality_gate={"result": "pass", "blocking_failures": [], "warnings": []},
        report_markdown="# Report\n",
    )
    Path(output / "report.md").write_text("# Tampered\n", encoding="utf-8")

    result = verify_run_pack_attestation(output)

    assert result["valid"] is False
    assert "report.md" in result["mismatched_artifacts"]
