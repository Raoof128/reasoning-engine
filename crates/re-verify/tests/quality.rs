//! Source quality in the pipeline (plan Phase 9): profile-aware rescoring,
//! and retracted work flagged but never supporting a claim.

use std::path::Path;
use std::sync::Arc;

use re_model::records::QualityHints;
use re_retrieval::base::{RecordInput, build_record};
use re_retrieval::sources::BoxFuture;
use re_retrieval::{AcademicSearchAggregator, RetrievalResult, SourceAdapter};
use re_store::Store;
use re_verify::{LexicalOverlap, VerifiableResearchService};
use serde_json::Value;

const DRAFT: &str = "Vitamin D supplements prevent hip fractures in older adults.";

/// One retracted article that matches the draft word for word, a preprint
/// and a systematic review.
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
        let record = |rank: usize, title: &str, snippet: &str, score: f64, hints: QualityHints| {
            let mut record = build_record(RecordInput {
                title: title.into(),
                snippet: snippet.into(),
                score,
                ..RecordInput::new(run_id, "openalex", query, rank)
            });
            record.hints = hints;
            record
        };
        let types = |types: &[&str]| QualityHints {
            publication_types: types.iter().map(|t| (*t).to_string()).collect(),
            ..QualityHints::default()
        };
        Box::pin(async move {
            RetrievalResult::ok(vec![
                record(
                    1,
                    "Vitamin D and hip fractures",
                    DRAFT,
                    0.9,
                    QualityHints {
                        retracted: Some(true),
                        ..types(&["article"])
                    },
                ),
                record(
                    2,
                    "Sleep and recovery",
                    "Sleep duration predicts recovery.",
                    0.8,
                    types(&["preprint"]),
                ),
                record(
                    3,
                    "Exercise and falls",
                    "Exercise programmes reduce falls.",
                    0.75,
                    types(&["Systematic Review"]),
                ),
            ])
        })
    }
}

fn service(dir: &Path, quality: bool) -> VerifiableResearchService {
    let service = VerifiableResearchService::new(
        Store::open(dir.join("engine.db")).unwrap(),
        dir.join("runs"),
        Arc::new(AcademicSearchAggregator::new(vec![Arc::new(Provider)])),
        Box::new(LexicalOverlap),
    );
    if quality {
        service.with_quality()
    } else {
        service
    }
}

/// The pipeline's evidence and its one claim verification.
async fn run(dir: &Path, quality: bool) -> (Vec<Value>, Value) {
    let result = service(dir, quality)
        .run_research_pipeline(
            "vitamin d hip fractures",
            DRAFT,
            "standard",
            "general",
            false,
        )
        .await
        .unwrap();
    let pack = Path::new(result["run_pack"].as_str().unwrap());
    let verifications: Value = serde_json::from_str(
        &std::fs::read_to_string(pack.join("claim_verifications.json")).unwrap(),
    )
    .unwrap();
    (
        result["evidence"].as_array().unwrap().clone(),
        verifications[0].clone(),
    )
}

#[tokio::test]
async fn without_quality_the_retracted_article_supports_the_claim() {
    let dir = tempfile::tempdir().unwrap();
    let (evidence, verification) = run(dir.path(), false).await;
    assert_eq!(
        verification["support_status"], "supported",
        "{verification}"
    );
    assert!(
        evidence
            .iter()
            .all(|e| e["risk_flags"].as_array().unwrap().is_empty()),
        "Python's labels: no quality flags"
    );
}

#[tokio::test]
async fn with_quality_retracted_work_is_flagged_and_cannot_support() {
    let dir = tempfile::tempdir().unwrap();
    let (evidence, verification) = run(dir.path(), true).await;
    assert_ne!(
        verification["support_status"], "supported",
        "{verification}"
    );
    assert!(
        !verification["evidence_ids"]
            .as_array()
            .unwrap()
            .iter()
            .any(|id| id == "ev_0001"),
        "{verification}"
    );

    assert_eq!(
        evidence[0]["score"], 1.0,
        "0.9 plus the preferred-type bonus"
    );
    let summary: Vec<(&str, &str, bool)> = evidence
        .iter()
        .map(|e| {
            (
                e["title"].as_str().unwrap(),
                e["source_type"].as_str().unwrap(),
                e["risk_flags"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|f| f == "retracted"),
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            ("Vitamin D and hip fractures", "peer_reviewed_article", true),
            ("Sleep and recovery", "preprint", false),
            ("Exercise and falls", "systematic_review", false),
        ]
    );
}
