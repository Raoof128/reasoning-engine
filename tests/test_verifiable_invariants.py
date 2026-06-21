import json
from pathlib import Path

from reasoning_engine.db import init_db
from reasoning_engine.server import get_scholar_auth_status
from reasoning_engine.verifiable.claims import extract_claims, verify_claims
from reasoning_engine.verifiable.quality import run_quality_gate
from reasoning_engine.verifiable.runpack import verify_run_pack_attestation
from reasoning_engine.verifiable.service import VerifiableResearchService


def test_failed_retrieval_creates_no_evidence(tmp_path, db_path):
    init_db(db_path)
    service = VerifiableResearchService(db_path=db_path, runs_dir=tmp_path)

    result = service.run_research_pipeline(
        query="unsupported retrieval path",
        draft="Unsupported retrieval path has evidence.",
        profile="general",
        use_mock_empty=True,
    )

    assert result["evidence"] == []


def test_unsupported_factual_claim_blocks_final_report():
    claims = extract_claims("run_001", "Scholar Gateway exposes semantic search.", "general")
    verifications = verify_claims(claims, [])

    gate = run_quality_gate("run_001", claims, verifications, gaps=[])

    assert gate["result"] == "blocked"


def test_token_value_never_appears_in_auth_status(monkeypatch):
    monkeypatch.setenv("SCHOLAR_GATEWAY_ACCESS_TOKEN", "secret-token-value")

    payload = json.loads(get_scholar_auth_status())

    assert payload["has_env_token"] is True
    assert "secret-token-value" not in json.dumps(payload)


def test_tampered_run_pack_fails_verification(tmp_path, db_path):
    init_db(db_path)
    service = VerifiableResearchService(db_path=db_path, runs_dir=tmp_path)
    result = service.run_research_pipeline(
        query="Scholar Gateway exposes semantic search",
        draft="Scholar Gateway exposes semantic search.",
        profile="general",
    )
    report = Path(result["run_pack"]) / "report.md"
    report.write_text("# Tampered\n", encoding="utf-8")

    assert verify_run_pack_attestation(result["run_pack"])["valid"] is False


def test_final_report_has_run_id_and_quality_gate(tmp_path, db_path):
    init_db(db_path)
    service = VerifiableResearchService(db_path=db_path, runs_dir=tmp_path)
    result = service.run_research_pipeline(
        query="Scholar Gateway exposes semantic search",
        draft="Scholar Gateway exposes semantic search.",
        profile="general",
    )

    report = Path(result["run_pack"]) / "report.md"
    text = report.read_text(encoding="utf-8")

    assert result["run_id"] in text
    assert "Quality gate:" in text
