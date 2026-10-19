//! Contradiction search in the pipeline (plan Phase 10): a supported claim
//! that a counter-search contests is flagged and listed in the report's
//! counter-view; one that survives gains confidence.
#![cfg(feature = "graph")]

use std::path::Path;
use std::sync::Arc;

use re_model::records::{ClaimRecord, EvidenceRecord, VerificationRecord};
use re_retrieval::base::{RecordInput, build_record};
use re_retrieval::sources::BoxFuture;
use re_retrieval::{AcademicSearchAggregator, RetrievalResult, SourceAdapter};
use re_store::Store;
use re_verify::{LexicalOverlap, VerifiableResearchService, Verifier};
use serde_json::Value;

const CONTESTED: &str = "Daytime naps improve afternoon power.";
const SURVIVOR: &str = "Daytime naps reduce muscle soreness.";
const CONTRA: &str = "No power benefit from napping was found.";

/// The pipeline's search finds both claims; the contradiction search for
/// the first finds a paper against it, for the second nothing.
struct Provider;

impl SourceAdapter for Provider {
    fn name(&self) -> &'static str {
        "openalex"
    }

    fn search<'a>(
        &'a self,
        run_id: &'a str,
        query: &'a str,
        _: usize,
    ) -> BoxFuture<'a, RetrievalResult> {
        let record = |rank: usize, title: &str, snippet: &str| {
            build_record(RecordInput {
                title: title.into(),
                snippet: snippet.into(),
                score: 0.9,
                ..RecordInput::new(run_id, "openalex", query, rank)
            })
        };
        let evidence = if query.starts_with("Daytime naps do not improve") {
            vec![record(1, "Naps show no power benefit", CONTRA)]
        } else if query.starts_with("Daytime naps do not") {
            Vec::new()
        } else {
            vec![
                record(1, "Napping and power", CONTESTED),
                record(2, "Napping and soreness", SURVIVOR),
            ]
        };
        Box::pin(async move { RetrievalResult::ok(evidence) })
    }
}

/// Supported when a snippet repeats the claim (ignoring case and the full
/// stop), contradicted by [`CONTRA`].
struct Scripted;

impl Verifier for Scripted {
    fn method(&self) -> &'static str {
        "scripted"
    }

    fn can_contradict(&self) -> bool {
        true
    }

    fn verify(
        &self,
        claims: &mut [ClaimRecord],
        evidence: &[EvidenceRecord],
    ) -> Vec<VerificationRecord> {
        claims
            .iter_mut()
            .map(|claim| {
                let against: Vec<String> = evidence
                    .iter()
                    .filter(|e| e.snippet == CONTRA)
                    .map(|e| e.evidence_id.clone())
                    .collect();
                let support: Vec<String> = evidence
                    .iter()
                    .filter(|e| {
                        let core = |t: &str| t.trim().trim_end_matches('.').to_lowercase();
                        core(&e.snippet) == core(&claim.text)
                    })
                    .map(|e| e.evidence_id.clone())
                    .collect();
                let status = if !against.is_empty() {
                    "contradicted"
                } else if support.is_empty() {
                    "unsupported"
                } else {
                    "supported"
                };
                claim.status = status.into();
                claim.confidence = Some(0.8);
                VerificationRecord {
                    verification_id: format!("ver_{}", claim.claim_id),
                    claim_id: claim.claim_id.clone(),
                    method: "scripted".into(),
                    evidence_ids: support,
                    support_status: status.into(),
                    support_rationale: "Scripted.".into(),
                    missing_evidence: None,
                    contradictory_evidence_ids: against,
                    confidence: 0.8,
                    requires_human_review: status != "supported",
                    verified_at: "2026-10-19T00:00:00Z".into(),
                }
            })
            .collect()
    }
}

fn service(dir: &Path, verifier: Box<dyn Verifier>) -> VerifiableResearchService {
    VerifiableResearchService::new(
        Store::open(dir.join("e.db")).unwrap(),
        dir.join("runs"),
        Arc::new(AcademicSearchAggregator::new(vec![Arc::new(Provider)])),
        verifier,
    )
    .with_counter_search()
}

#[tokio::test]
async fn contested_claims_are_flagged_and_survivors_gain_confidence() {
    let dir = tempfile::tempdir().unwrap();
    let service = service(dir.path(), Box::new(Scripted));
    let result = service
        .run_research_pipeline(
            "daytime naps",
            &format!("{CONTESTED} {SURVIVOR}"),
            "standard",
            "general",
            false,
        )
        .await
        .unwrap();
    let pack = Path::new(result["run_pack"].as_str().unwrap());
    let verifications: Vec<Value> = serde_json::from_str(
        &std::fs::read_to_string(pack.join("claim_verifications.json")).unwrap(),
    )
    .unwrap();

    let contested = &verifications[0];
    assert_eq!(contested["support_status"], "supported");
    assert_eq!(contested["requires_human_review"], true);
    assert_eq!(contested["confidence"], 0.8);
    assert_eq!(
        contested["support_rationale"],
        "Scripted. Contradiction search found contrary work: Naps show no power benefit."
    );

    let survivor = &verifications[1];
    assert_eq!(survivor["confidence"], 0.85);
    assert_eq!(survivor["requires_human_review"], false);
    assert_eq!(
        survivor["support_rationale"],
        "Scripted. Survived contradiction search."
    );

    let report = std::fs::read_to_string(pack.join("report.md")).unwrap();
    assert!(
        report.ends_with(&format!(
            "## Counter-view\n\n- [supported, contested] {}: contrary: \
             Naps show no power benefit (openalex)\n",
            CONTESTED.trim_end_matches('.')
        )),
        "{report}"
    );

    let run_id = result["run_id"].as_str().unwrap();
    let event = service
        .store()
        .list_provenance(run_id)
        .unwrap()
        .into_iter()
        .find(|e| e["event_type"] == "counter_search")
        .expect("a counter_search event");
    let queries: Vec<&str> = event["searches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["query"].as_str().unwrap())
        .collect();
    assert_eq!(
        queries,
        [
            "Daytime naps do not improve afternoon power",
            "Daytime naps do not reduce muscle soreness"
        ]
    );
}

#[tokio::test]
async fn tiers_that_cannot_contradict_skip_the_search() {
    let dir = tempfile::tempdir().unwrap();
    let service = service(dir.path(), Box::new(LexicalOverlap));
    let result = service
        .run_research_pipeline("daytime naps", CONTESTED, "standard", "general", false)
        .await
        .unwrap();
    let run_id = result["run_id"].as_str().unwrap();
    assert!(
        service
            .store()
            .list_provenance(run_id)
            .unwrap()
            .iter()
            .all(|e| e["event_type"] != "counter_search")
    );
    let pack = Path::new(result["run_pack"].as_str().unwrap());
    assert!(
        !std::fs::read_to_string(pack.join("report.md"))
            .unwrap()
            .contains("Counter-view")
    );
}
