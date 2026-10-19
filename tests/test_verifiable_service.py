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


def test_two_pipelines_on_one_store_keep_both_runs_claims(tmp_path, db_path):
    init_db(db_path)
    service = VerifiableResearchService(db_path=db_path, runs_dir=tmp_path)

    run_ids = [
        service.run_research_pipeline(
            query="Scholar Gateway exposes semantic search",
            draft=draft,
            mode="standard",
            profile="general",
        )["run_id"]
        for draft in [
            "Scholar Gateway exposes semantic search. Teams should audit tools.",
            "Scholar Gateway exposes semantic search.",
        ]
    ]

    # Both runs number their claims from claim_0001; neither may overwrite
    # the other's rows.
    for run_id, expected in zip(run_ids, [2, 1], strict=True):
        claims = service.store.list_claims(run_id)
        assert len(claims) == expected
        assert claims[0]["claim_id"] == "claim_0001"
        assert all(claim["run_id"] == run_id for claim in claims)
        assert len(service.store.list_verifications(run_id)) == expected


def test_repeated_empty_searches_record_distinct_gaps(tmp_path, db_path):
    init_db(db_path)
    service = VerifiableResearchService(db_path=db_path, runs_dir=tmp_path)

    run = service.start_run(query="No evidence exists", mode="standard", profile="general")
    service.scholar_search(run.run_id, run.query, use_mock_empty=True)
    service.scholar_search(run.run_id, run.query, use_mock_empty=True)

    gaps = service.store.list_gaps(run.run_id)
    assert len(gaps) == 2
    assert len({gap["gap_id"] for gap in gaps}) == 2


def test_service_records_provenance_in_chronological_order(tmp_path, db_path):
    init_db(db_path)
    service = VerifiableResearchService(db_path=db_path, runs_dir=tmp_path)

    run = service.start_run(
        query="Scholar Gateway exposes semantic search",
        mode="standard",
        profile="general",
    )
    service.scholar_search(run.run_id, run.query)

    events = [event["event_type"] for event in service.store.list_provenance(run.run_id)]

    # All events land in the same wall-clock second; ordering must still follow
    # insertion order, with run_created first, not the hash-based event_id.
    assert events == [
        "run_created",
        "profile_selected",
        "mode_selected",
        "scholar_search_requested",
        "scholar_search_completed",
    ]
