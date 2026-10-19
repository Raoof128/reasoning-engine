//! Deterministic offline adapter for `REASONING_ENGINE_OFFLINE=1`, tests and
//! CI (port of `mock.py`).

use re_model::clock::utc_now;
use re_model::records::{EvidenceRecord, RetrievalError};
use serde_json::{Map, Value};

use super::{BoxFuture, SourceAdapter};
use crate::base::py_strip;
use crate::result::RetrievalResult;

/// Name shared with the live Scholar Gateway adapter it stands in for.
pub const NAME: &str = "scholar_gateway";

/// Returns `limit` fixed, clearly labelled mock records.
#[derive(Debug, Clone, Copy, Default)]
pub struct MockScholarGateway;

impl MockScholarGateway {
    /// Synchronous search; the trait method wraps it.
    #[must_use]
    pub fn search_now(run_id: &str, query: &str, limit: usize) -> RetrievalResult {
        let query = py_strip(query);
        if query.is_empty() {
            return RetrievalResult::failed(RetrievalError {
                error_type: "unsupported_query".into(),
                message: "query must not be empty".into(),
                retryable: false,
                metadata: Map::new(),
            });
        }
        let evidence = (1..=limit)
            .map(|index| {
                let mut metadata = Map::new();
                metadata.insert("mock".into(), Value::Bool(true));
                EvidenceRecord {
                    hints: re_model::records::QualityHints::default(),
                    evidence_id: format!("ev_{index:04}"),
                    run_id: run_id.to_string(),
                    source_adapter: NAME.into(),
                    source_type: "peer_reviewed_article".into(),
                    title: format!("Mock Scholar Gateway Result {index}"),
                    authors: vec!["Mock Author".into()],
                    year: Some(2026),
                    publisher: "Wiley".into(),
                    venue: "Mock Journal".into(),
                    doi: Some(format!("10.1000/mock.{index}")),
                    url: Some(format!("https://doi.org/10.1000/mock.{index}")),
                    retrieved_at: utc_now(),
                    query: query.to_string(),
                    rank: i64::try_from(index).unwrap_or(i64::MAX),
                    score: (1.0 - (index - 1) as f64 * 0.05).max(0.0),
                    snippet: format!("Mock evidence for {query}."),
                    licence_notes: Some("Mock fixture for tests.".into()),
                    risk_flags: Vec::new(),
                    metadata,
                }
            })
            .collect();
        RetrievalResult::ok(evidence)
    }
}

impl SourceAdapter for MockScholarGateway {
    fn name(&self) -> &'static str {
        NAME
    }

    fn search<'a>(
        &'a self,
        run_id: &'a str,
        query: &'a str,
        limit: usize,
    ) -> BoxFuture<'a, RetrievalResult> {
        Box::pin(async move { Self::search_now(run_id, query, limit) })
    }
}
