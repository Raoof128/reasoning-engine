//! The pipeline with the `trust` feature: numeric consistency and styled
//! references inside the attested pack.
#![cfg(feature = "trust")]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use re_retrieval::{AcademicSearchAggregator, adapters_from};
use re_store::Store;
use re_verify::{LexicalOverlap, VerifiableResearchService};
use serde_json::Value;

fn service(root: &Path) -> VerifiableResearchService {
    // The offline mock's snippet is "Mock evidence for {query}.", so the
    // query decides which numbers the evidence contains.
    let adapters =
        adapters_from(|var| (var == "REASONING_ENGINE_OFFLINE").then(|| "1".into())).unwrap();
    VerifiableResearchService::new(
        Store::open(root.join("engine.db")).unwrap(),
        root.join("runs"),
        Arc::new(AcademicSearchAggregator::new(adapters)),
        Box::new(LexicalOverlap),
    )
}

async fn run(service: &VerifiableResearchService, query: &str, draft: &str) -> (Value, PathBuf) {
    let result = service
        .run_research_pipeline(query, draft, "standard", "auto", false)
        .await
        .unwrap();
    let pack = PathBuf::from(result["run_pack"].as_str().unwrap());
    let verifications: Value = serde_json::from_str(
        &std::fs::read_to_string(pack.join("claim_verifications.json")).unwrap(),
    )
    .unwrap();
    (verifications[0].clone(), pack)
}

#[tokio::test]
async fn a_number_missing_from_the_evidence_caps_support() {
    let root = tempfile::tempdir().unwrap();
    let service = service(root.path());
    let draft = "Scholar Gateway semantic search improves recall by 37%.";

    let (found, _) = run(
        &service,
        "Scholar Gateway semantic search improves recall by 37%",
        draft,
    )
    .await;
    assert_eq!(found["support_status"], "supported", "{found}");

    let (missing, pack) = run(
        &service,
        "Scholar Gateway semantic search improves recall",
        draft,
    )
    .await;
    assert_eq!(
        missing["support_status"], "partially_supported",
        "{missing}"
    );
    assert_eq!(
        missing["missing_evidence"],
        "numeric value not found in evidence"
    );
    assert_eq!(missing["requires_human_review"], true);
    let claims = std::fs::read_to_string(pack.join("claims.json")).unwrap();
    assert!(claims.contains("\"status\": \"partially_supported\""));
}

#[tokio::test]
async fn an_invented_number_on_weak_evidence_is_unsupported_and_blocks() {
    let root = tempfile::tempdir().unwrap();
    let query = "Retrieval augmented generation reduces hallucination";
    let draft =
        "Retrieval augmented generation eliminates 100% of hallucinations in every language model.";
    let result = service(root.path())
        .run_research_pipeline(query, draft, "standard", "auto", false)
        .await
        .unwrap();
    let pack = PathBuf::from(result["run_pack"].as_str().unwrap());
    let verifications: Value = serde_json::from_str(
        &std::fs::read_to_string(pack.join("claim_verifications.json")).unwrap(),
    )
    .unwrap();
    let verdict = &verifications[0];
    assert_eq!(verdict["support_status"], "unsupported", "{verdict}");
    assert_eq!(
        verdict["missing_evidence"],
        "numeric value not found in evidence"
    );
    assert_eq!(verdict["evidence_ids"], serde_json::json!([]));
    assert_eq!(result["quality_gate"]["result"], "blocked", "{result}");
}

#[tokio::test]
async fn packs_carry_an_attested_reference_list_in_the_profile_style() {
    let root = tempfile::tempdir().unwrap();
    let (_, pack) = run(
        &service(root.path()),
        "Scholar Gateway exposes semantic search",
        "Scholar Gateway exposes semantic search.",
    )
    .await;
    let references = std::fs::read_to_string(pack.join("references.harvard_au.md")).unwrap();
    assert!(references.starts_with("# References\n\nStyle: Harvard (Cite Them Right)"));
    assert!(references.contains("Mock Scholar Gateway Result"));
    assert!(pack.join("sources.bib").exists(), "kept for compatibility");
    let attestation: Value =
        serde_json::from_str(&std::fs::read_to_string(pack.join("attestation.json")).unwrap())
            .unwrap();
    assert!(attestation["artifact_hashes"]["references.harvard_au.md"].is_string());
    let verified = re_verify::runpack::verify_run_pack_attestation(&pack);
    assert_eq!(verified["valid"], true);
    std::fs::write(pack.join("references.harvard_au.md"), "forged").unwrap();
    assert_eq!(
        re_verify::runpack::verify_run_pack_attestation(&pack)["valid"],
        false
    );
}
