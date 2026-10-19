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
