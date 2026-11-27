//! The offline research pipeline end to end, mirroring the Python service
//! tests, plus Python verifying the pack the Rust pipeline wrote.

mod common;

use std::ffi::OsStr;
use std::path::Path;
use std::sync::Arc;

use common::verify_tool;
use re_retrieval::AcademicSearchAggregator;
use re_retrieval::sources::mock::MockScholarGateway;
use re_store::Store;
use re_verify::{LexicalOverlap, VerifiableResearchService};
use serde_json::Value;

fn service(dir: &Path) -> VerifiableResearchService {
    let store = Store::open(dir.join("reasoning.db")).expect("store");
    let search = Arc::new(AcademicSearchAggregator::new(vec![Arc::new(
        MockScholarGateway,
    )]));
    VerifiableResearchService::new(store, dir.join("runs"), search, Box::new(LexicalOverlap))
}

fn event_types(service: &VerifiableResearchService, run_id: &str) -> Vec<String> {
    service
        .store()
        .list_provenance(run_id)
        .expect("provenance")
        .iter()
        .map(|e| e["event_type"].as_str().expect("type").to_string())
        .collect()
}

#[tokio::test]
async fn offline_pipeline_exports_a_pack_python_accepts() {
    let dir = tempfile::tempdir().expect("tmp");
    let service = service(dir.path());
    let result = service
        .run_research_pipeline(
            "MCP prompt injection defences",
            "Mock evidence for MCP prompt injection defences. Teams should audit tools.",
            "standard",
            "auto",
            false,
        )
        .await
        .expect("pipeline");

    let run_id = result["run_id"].as_str().expect("run id");
    assert_eq!(result["attestation"]["valid"], true);
    assert_eq!(result["evidence"].as_array().expect("evidence").len(), 10);
    assert_eq!(
        result["verified_against_snippets"][0],
        "Mock evidence for MCP prompt injection defences."
    );
    assert_eq!(result["quality_gate"]["result"], "pass");
    let keys: Vec<&String> = result.as_object().expect("object").keys().collect();
    assert_eq!(
        keys,
        [
            "run_id",
            "quality_gate",
            "run_pack",
            "attestation",
            "evidence",
            "verified_against_snippets"
        ]
    );

    let pack = result["run_pack"].as_str().expect("path");
    assert!(Path::new(pack).ends_with(run_id));
    let by_python = verify_tool(&[OsStr::new("verify"), OsStr::new(pack)]);
    assert_eq!(by_python["valid"], true, "{by_python}");

    assert_eq!(
        event_types(&service, run_id),
        [
            "run_created",
            "profile_selected",
            "mode_selected",
            "scholar_search_requested",
            "scholar_search_completed"
        ]
    );
    let claims = service.store().list_claims(run_id).expect("claims");
    assert_eq!(claims[0]["status"], "supported");
    assert_eq!(claims[1]["status"], "not_verifiable");
}

#[tokio::test]
async fn two_pipelines_on_one_store_keep_both_runs_evidence_and_claims() {
    let dir = tempfile::tempdir().expect("tmp");
    let service = service(dir.path());
    let mut runs = Vec::new();
    for draft in [
        "Mock evidence for MCP prompt injection defences. Teams should audit tools.",
        "Mock evidence for MCP prompt injection defences.",
    ] {
        let result = service
            .run_research_pipeline(
                "MCP prompt injection defences",
                draft,
                "standard",
                "auto",
                false,
            )
            .await
            .expect("pipeline");
        let evidence = result["evidence"].as_array().expect("evidence").len();
        runs.push((
            result["run_id"].as_str().expect("run id").to_string(),
            evidence,
        ));
    }

    // Both runs number their evidence from ev_0001 and their claims from
    // claim_0001; neither may overwrite the other's rows.
    let store = service.store();
    for ((run_id, evidence_count), expected) in runs.iter().zip([2, 1]) {
        let evidence = store.list_evidence(run_id).expect("evidence");
        assert_eq!(evidence.len(), *evidence_count, "{run_id}");
        assert_eq!(evidence[0]["evidence_id"], "ev_0001");
        assert!(evidence.iter().all(|e| e["run_id"] == run_id.as_str()));
        let claims = store.list_claims(run_id).expect("claims");
        assert_eq!(claims.len(), expected, "{run_id}");
        assert_eq!(claims[0]["claim_id"], "claim_0001");
        assert!(claims.iter().all(|c| c["run_id"] == run_id.as_str()));
        let verifications = store.list_verifications(run_id).expect("verifications");
        assert_eq!(verifications.len(), expected, "{run_id}");
    }
    assert_eq!(
        store.count_rows("evidence_records").expect("count"),
        i64::try_from(runs[0].1 + runs[1].1).expect("count")
    );
    assert_eq!(store.count_rows("claims").expect("count"), 3);
}

#[tokio::test]
async fn empty_search_records_a_gap_and_blocks_cited_claims() {
    let dir = tempfile::tempdir().expect("tmp");
    let service = service(dir.path());
    let result = service
        .run_research_pipeline("q", "Water boils at 100 degrees.", "standard", "auto", true)
        .await
        .expect("pipeline");
    assert_eq!(result["quality_gate"]["result"], "blocked");
    assert_eq!(
        result["quality_gate"]["warnings"][0],
        "unresolved evidence gaps are present"
    );
    let pack = Path::new(result["run_pack"].as_str().expect("path"));
    let gaps = std::fs::read_to_string(pack.join("unresolved_gaps.md")).expect("gaps");
    assert_eq!(gaps, "- No evidence found.\n");
    let report = std::fs::read_to_string(pack.join("report.md")).expect("report");
    assert!(
        report.ends_with("## Claims\n- [needs_more_evidence] Water boils at 100 degrees\n"),
        "{report}"
    );
}

#[tokio::test]
async fn search_errors_become_gaps_with_provenance() {
    let dir = tempfile::tempdir().expect("tmp");
    let service = service(dir.path());
    let run = service.start_run("q", "standard", "auto").expect("run");
    // The mock rejects a blank query, like every adapter.
    let outcome = service
        .scholar_search(&run.run_id, "   ", 5, false)
        .await
        .expect("search");
    let json = outcome.to_json();
    assert_eq!(json["error"]["error_type"], "unsupported_query");
    assert_eq!(json["gaps"][0]["reason"], "query must not be empty");
    assert_eq!(json["evidence"], Value::Array(Vec::new()));
    assert_eq!(
        event_types(&service, &run.run_id)
            .last()
            .map(String::as_str),
        Some("evidence_gap_recorded")
    );
}

#[test]
fn start_run_escalates_and_rejects_like_python() {
    let dir = tempfile::tempdir().expect("tmp");
    let service = service(dir.path());
    let run = service
        .start_run("clinical trial dosage for patients", "standard", "auto")
        .expect("run");
    assert_eq!(
        (run.profile.as_str(), run.mode.as_str()),
        ("medicine", "high_stakes")
    );
    let error = service
        .start_run("q", "standard", "astrology")
        .expect_err("unknown profile");
    assert!(error.to_string().contains("astrology"), "{error}");
    assert!(service.start_run("q", "turbo", "auto").is_err());
}

/// Returns `limit` records whose DOI and snippet depend on the query, so
/// two different searches find different papers.
struct PerQuery;

impl re_retrieval::SourceAdapter for PerQuery {
    fn name(&self) -> &'static str {
        "per_query"
    }

    fn search<'a>(
        &'a self,
        run_id: &'a str,
        query: &'a str,
        limit: usize,
    ) -> re_retrieval::sources::BoxFuture<'a, re_retrieval::RetrievalResult> {
        Box::pin(async move {
            let mut result = MockScholarGateway::search_now(run_id, query, limit);
            let slug: String = query
                .chars()
                .filter(|c| c.is_alphanumeric())
                .collect::<String>()
                .to_lowercase();
            for (record, number) in result.evidence.iter_mut().zip(1..) {
                record.doi = Some(format!("10.1000/{slug}.{number}"));
                record.title = format!("{query} paper {number}");
                record.snippet = format!("{query} improve accuracy on long tasks.");
            }
            result
        })
    }
}

fn per_query_service(dir: &Path) -> VerifiableResearchService {
    let store = Store::open(dir.join("reasoning.db")).expect("store");
    let search = Arc::new(AcademicSearchAggregator::new(vec![Arc::new(PerQuery)]));
    VerifiableResearchService::new(store, dir.join("runs"), search, Box::new(LexicalOverlap))
}

fn ids(outcome: &re_verify::service::SearchOutcome) -> Vec<String> {
    outcome
        .evidence_records
        .iter()
        .map(|e| e.evidence_id.clone())
        .collect()
}

#[tokio::test]
async fn searches_in_one_run_accumulate_evidence() {
    // Each search numbers its results from ev_0001 and records are stored by
    // (run, id): the second search used to replace the first one's evidence.
    let dir = tempfile::tempdir().expect("tmp");
    let service = per_query_service(dir.path());
    let run = service
        .start_run("reward models", "standard", "auto")
        .expect("run");
    let first = service
        .scholar_search(&run.run_id, "process reward models", 3, false)
        .await
        .expect("first search");
    let second = service
        .scholar_search(&run.run_id, "outcome reward models", 3, false)
        .await
        .expect("second search");
    assert_eq!(ids(&first), ["ev_0001", "ev_0002", "ev_0003"]);
    assert_eq!(ids(&second), ["ev_0004", "ev_0005", "ev_0006"]);
    let stored = service.store().list_evidence(&run.run_id).expect("stored");
    assert_eq!(stored.len(), 6);
    assert!(
        stored[0]["snippet"]
            .as_str()
            .expect("snippet")
            .starts_with("process")
    );

    // Asking again finds the same papers under the IDs they already have.
    let again = service
        .scholar_search(&run.run_id, "process reward models", 3, false)
        .await
        .expect("repeat search");
    assert_eq!(ids(&again), ids(&first));
    assert_eq!(
        service
            .store()
            .list_evidence(&run.run_id)
            .expect("stored")
            .len(),
        6
    );
}

#[tokio::test]
async fn pipeline_verifies_against_the_evidence_a_run_gathered() {
    let dir = tempfile::tempdir().expect("tmp");
    let service = per_query_service(dir.path());
    let run = service
        .start_run("reward models", "standard", "auto")
        .expect("run");
    for query in ["process reward models", "outcome reward models"] {
        service
            .scholar_search(&run.run_id, query, 3, false)
            .await
            .expect("search");
    }
    let draft = "# Findings\n\n## What we saw\n\nProcess reward models improve accuracy on long \
                 tasks. Quantum annealing cures insomnia in mice.\n\n| a | b |\n| --- | --- |\n\n\
                 ## Sources\n\n1. Someone. A paper. https://doi.org/10.1/x\n";
    let result = service
        .run_research_pipeline_from(
            "reward models",
            draft,
            "standard",
            "auto",
            false,
            Some(&run.run_id),
        )
        .await
        .expect("pipeline");

    assert_eq!(result["evidence_reused_from"], run.run_id.as_str());
    assert_eq!(result["evidence_count"], 6);
    assert_eq!(result["attestation"]["valid"], true);
    // The evidence is counted, not repeated.
    assert!(result.get("evidence").is_none());
    assert!(result.get("verified_against_snippets").is_none());

    // Two claims: the heading, the table and the source list are not claims.
    let summary = &result["summary"];
    assert_eq!(summary["claims"], 2);
    assert_eq!(summary["by_status"]["supported"], 1);
    let attention = summary["needs_attention"].as_array().expect("list");
    assert_eq!(attention.len(), 1);
    assert!(
        attention[0]["text"]
            .as_str()
            .expect("text")
            .starts_with("Quantum annealing")
    );
    assert_eq!(result["quality_gate"]["result"], "blocked");

    // The new run reused evidence and did not search again.
    let new_run = result["run_id"].as_str().expect("run id");
    let events = event_types(&service, new_run);
    assert!(
        events.contains(&"evidence_reused".to_string()),
        "{events:?}"
    );
    assert!(
        !events.contains(&"scholar_search_requested".to_string()),
        "{events:?}"
    );
    assert_eq!(
        service
            .store()
            .list_evidence(new_run)
            .expect("stored")
            .len(),
        6
    );
}

#[tokio::test]
async fn reuse_falls_back_to_searching_and_rejects_unknown_runs() {
    let dir = tempfile::tempdir().expect("tmp");
    let service = per_query_service(dir.path());
    // A run that never searched holds no evidence: the pipeline searches.
    let empty = service
        .start_run("reward models", "standard", "auto")
        .expect("run");
    let result = service
        .run_research_pipeline_from(
            "process reward models",
            "Process reward models improve accuracy on long tasks.",
            "standard",
            "auto",
            false,
            Some(&empty.run_id),
        )
        .await
        .expect("pipeline");
    assert!(result["evidence_reused_from"].is_null());
    assert_eq!(result["evidence_count"], 10);
    assert_eq!(result["summary"]["by_status"]["supported"], 1);

    let unknown = service
        .run_research_pipeline_from("q", "A claim.", "standard", "auto", false, Some("run_nope"))
        .await;
    assert!(unknown.is_err());
}
