//! Mirrors `tests/test_verifiable_store.py`.

use re_model::clock::utc_now;
use re_model::records::{ClaimRecord, EvidenceGap, EvidenceRecord, ResearchRun};
use re_store::Store;
use serde_json::{Map, Value, json};

fn store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("test.db")).unwrap();
    (dir, store)
}

fn evidence(run: &ResearchRun) -> EvidenceRecord {
    EvidenceRecord {
        hints: re_model::records::QualityHints::default(),
        evidence_id: "ev_0001".into(),
        run_id: run.run_id.clone(),
        source_adapter: "scholar_gateway".into(),
        source_type: "peer_reviewed_article".into(),
        title: "Gateway paper".into(),
        authors: vec!["A. Author".into()],
        year: Some(2026),
        publisher: "Wiley".into(),
        venue: "Journal".into(),
        doi: Some("10.1000/example".into()),
        url: Some("https://doi.org/10.1000/example".into()),
        retrieved_at: utc_now(),
        query: run.query.clone(),
        rank: 1,
        score: 0.9,
        snippet: "Scholar Gateway exposes semantic search.".into(),
        licence_notes: None,
        risk_flags: vec![],
        metadata: Map::new(),
    }
}

#[test]
fn store_round_trips_run_evidence_gap_and_claim() {
    let (_d, s) = store();
    let run = ResearchRun::create("Explain Scholar Gateway", "general", "standard");
    let gap = EvidenceGap {
        gap_id: "gap_0001".into(),
        run_id: run.run_id.clone(),
        query: run.query.clone(),
        reason: "No second corroborating source found.".into(),
        created_at: utc_now(),
    };
    let mut claim = ClaimRecord::new(
        "claim_0001",
        &run.run_id,
        "Scholar Gateway exposes semantic search.",
        "empirical",
        "general",
        "high",
        "low",
        true,
        "test",
    )
    .unwrap();
    claim.evidence_ids = vec!["ev_0001".into()];
    claim.status = "supported".into();

    s.save_run(&run).unwrap();
    s.save_evidence(&evidence(&run)).unwrap();
    s.save_gap(&gap).unwrap();
    s.save_claim(&claim).unwrap();

    assert_eq!(s.get_run(&run.run_id).unwrap().query, run.query);
    let listed = s.list_evidence(&run.run_id).unwrap();
    assert!(
        listed[0]["snippet_hash"]
            .as_str()
            .unwrap()
            .starts_with("sha256:")
    );
    assert_eq!(
        s.list_gaps(&run.run_id).unwrap()[0]["reason"],
        gap.reason.as_str()
    );
    assert_eq!(
        s.list_claims(&run.run_id).unwrap()[0]["status"],
        "supported"
    );
    assert_eq!(
        s.get_run("run_missing").unwrap_err().to_string(),
        "unknown research run: run_missing"
    );
}

#[test]
fn append_provenance_retains_identical_same_second_events() {
    let (_d, s) = store();
    let run = ResearchRun::create("Identical events", "general", "standard");
    s.save_run(&run).unwrap();
    let payload = json!({"query": "q", "limit": 10})
        .as_object()
        .unwrap()
        .clone();
    s.append_provenance(&run.run_id, "scholar_search_requested", &payload)
        .unwrap();
    s.append_provenance(&run.run_id, "scholar_search_requested", &payload)
        .unwrap();
    let events = s.list_provenance(&run.run_id).unwrap();
    assert_eq!(events.len(), 2);
    assert!(
        events
            .iter()
            .all(|e| e["event_type"] == "scholar_search_requested")
    );
}

#[test]
fn provenance_payload_keys_override_like_python() {
    let (_d, s) = store();
    let run = ResearchRun::create("Override", "general", "standard");
    s.save_run(&run).unwrap();
    let payload = json!({"timestamp": "fixed", "extra": 1})
        .as_object()
        .unwrap()
        .clone();
    s.append_provenance(&run.run_id, "custom", &payload)
        .unwrap();
    let event = &s.list_provenance(&run.run_id).unwrap()[0];
    assert_eq!(event["timestamp"], "fixed");
    assert_eq!(event["extra"], 1);
    assert_eq!(event["run_id"], Value::String(run.run_id.clone()));
}

#[test]
fn invalid_records_are_rejected_before_writing() {
    let (_d, s) = store();
    let run = ResearchRun::create("Bad", "general", "standard");
    s.save_run(&run).unwrap();
    let mut claim = ClaimRecord::new(
        "c1",
        &run.run_id,
        "t",
        "empirical",
        "g",
        "m",
        "l",
        true,
        "d",
    )
    .unwrap();
    claim.status = "vibes".into();
    assert_eq!(
        s.save_claim(&claim).unwrap_err().to_string(),
        "unknown support status: vibes"
    );
    assert_eq!(s.count_rows("claims").unwrap(), 0);
}
