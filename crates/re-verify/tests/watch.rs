//! Living reports (plan Phase 11): re-verifying a run after its source is
//! retracted reports the drift.
#![cfg(feature = "learn")]

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use re_retrieval::base::{RecordInput, build_record};
use re_retrieval::sources::BoxFuture;
use re_retrieval::{AcademicSearchAggregator, RetrievalResult, SourceAdapter};
use re_store::Store;
use re_verify::{LexicalOverlap, VerifiableResearchService};

const CLAIM: &str = "Daytime naps improve afternoon power in rugby athletes.";

/// One paper supporting the claim, retracted once `retracted` is set.
struct Provider {
    retracted: Arc<AtomicBool>,
}

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
        Box::pin(async move {
            let mut record = build_record(RecordInput {
                title: "Napping and afternoon power".into(),
                snippet: CLAIM.into(),
                score: 0.9,
                ..RecordInput::new(run_id, "openalex", query, 1)
            });
            record.doi = Some("10.1/naps".into());
            if self.retracted.load(Ordering::SeqCst) {
                record.hints.retracted = Some(true);
            }
            RetrievalResult::ok(vec![record])
        })
    }
}

#[tokio::test]
async fn a_retraction_after_the_run_is_reported_as_drift() {
    let dir = tempfile::tempdir().unwrap();
    let retracted = Arc::new(AtomicBool::new(false));
    let service = VerifiableResearchService::new(
        Store::open(dir.path().join("e.db")).unwrap(),
        dir.path().join("runs"),
        Arc::new(AcademicSearchAggregator::new(vec![Arc::new(Provider {
            retracted: Arc::clone(&retracted),
        })])),
        Box::new(LexicalOverlap),
    )
    .with_quality();
    let first = service
        .run_research_pipeline("daytime naps", CLAIM, "standard", "general", false)
        .await
        .unwrap();
    let run_id = first["run_id"].as_str().unwrap();

    // Nothing has changed yet.
    let unchanged = service.watch(run_id).await.unwrap();
    assert_eq!(unchanged["changed"], false, "{unchanged}");

    retracted.store(true, Ordering::SeqCst);
    let report = service.watch(run_id).await.unwrap();
    assert_eq!(report["changed"], true, "{report}");
    assert_ne!(report["new_run_id"], run_id);
    let claim = &report["drift"]["claims"][0];
    assert_eq!(claim["before"], "supported", "{report}");
    assert_ne!(claim["after"], "supported");
    assert_eq!(
        report["drift"]["flags"][0]["added"],
        serde_json::json!(["retracted"])
    );
    let path = Path::new(report["report"].as_str().unwrap());
    assert!(path.starts_with(dir.path().join("runs").join(run_id).join("watch")));
    let text = std::fs::read_to_string(path).unwrap();
    assert!(
        text.contains("- Napping and afternoon power: now retracted"),
        "{text}"
    );
    assert!(text.contains(": supported -> "), "{text}");

    let missing = service.watch("run_000000000000").await.unwrap_err();
    assert!(
        missing.to_string().contains("run_000000000000"),
        "{missing}"
    );
}
