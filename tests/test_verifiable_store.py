from reasoning_engine.db import init_db
from reasoning_engine.verifiable.models import (
    ClaimRecord,
    EvidenceGap,
    EvidenceRecord,
    ResearchRun,
    utc_now,
)
from reasoning_engine.verifiable.store import ResearchStore


def test_store_round_trips_run_evidence_gap_and_claim(db_path):
    init_db(db_path)
    store = ResearchStore(db_path)
    run = ResearchRun.create("Explain Scholar Gateway", profile="general", mode="standard")
    evidence = EvidenceRecord(
        evidence_id="ev_0001",
        run_id=run.run_id,
        source_adapter="scholar_gateway",
        source_type="peer_reviewed_article",
        title="Gateway paper",
        authors=["A. Author"],
        year=2026,
        publisher="Wiley",
        venue="Journal",
        doi="10.1000/example",
        url="https://doi.org/10.1000/example",
        retrieved_at=utc_now(),
        query=run.query,
        rank=1,
        score=0.9,
        snippet="Scholar Gateway exposes semantic search.",
        licence_notes=None,
        risk_flags=[],
        metadata={},
    )
    gap = EvidenceGap(
        gap_id="gap_0001",
        run_id=run.run_id,
        query=run.query,
        reason="No second corroborating source found.",
        created_at=utc_now(),
    )
    claim = ClaimRecord(
        claim_id="claim_0001",
        run_id=run.run_id,
        text="Scholar Gateway exposes semantic search.",
        claim_type="empirical",
        domain="general",
        importance="high",
        risk_level="low",
        requires_citation=True,
        created_from="test",
        evidence_ids=["ev_0001"],
        status="supported",
    )

    store.save_run(run)
    store.save_evidence(evidence)
    store.save_gap(gap)
    store.save_claim(claim)

    assert store.get_run(run.run_id)["query"] == run.query
    assert store.list_evidence(run.run_id)[0]["snippet_hash"].startswith("sha256:")
    assert store.list_gaps(run.run_id)[0]["reason"] == gap.reason
    assert store.list_claims(run.run_id)[0]["status"] == "supported"


def test_append_provenance_retains_identical_same_second_events(db_path):
    init_db(db_path)
    store = ResearchStore(db_path)
    run = ResearchRun.create("Identical events", profile="general", mode="standard")
    store.save_run(run)

    # Same type and payload, emitted back-to-back (same wall-clock second):
    # both must be retained, not collapsed by INSERT OR REPLACE.
    store.append_provenance(run.run_id, "scholar_search_requested", {"query": "q", "limit": 10})
    store.append_provenance(run.run_id, "scholar_search_requested", {"query": "q", "limit": 10})

    events = store.list_provenance(run.run_id)
    assert len(events) == 2
    assert all(event["event_type"] == "scholar_search_requested" for event in events)
