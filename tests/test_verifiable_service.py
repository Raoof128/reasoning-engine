from reasoning_engine.db import init_db
from reasoning_engine.verifiable.service import VerifiableResearchService


def test_service_runs_pipeline_and_blocks_unsupported_claim(tmp_path, db_path):
    init_db(db_path)
    service = VerifiableResearchService(db_path=db_path, runs_dir=tmp_path)

    result = service.run_research_pipeline(
        query="A claim with no matching evidence about unrelated material.",
        draft="A claim with no matching evidence about unrelated material.",
        mode="standard",
        profile="general",
        use_mock_empty=True,
    )

    assert result["quality_gate"]["result"] == "blocked"
    assert result["run_id"].startswith("run_")


def test_service_runs_pipeline_and_exports_attested_pack(tmp_path, db_path):
    init_db(db_path)
    service = VerifiableResearchService(db_path=db_path, runs_dir=tmp_path)

    result = service.run_research_pipeline(
        query="Scholar Gateway exposes semantic search",
        draft="Scholar Gateway exposes semantic search.",
        mode="standard",
        profile="general",
    )

    assert result["quality_gate"]["result"] == "pass"
    assert result["run_pack"]
    assert result["attestation"]["valid"] is True
    assert result["evidence"][0]["snippet"] in result["verified_against_snippets"]
