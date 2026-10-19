//! Result container shared by adapters and the aggregator.

use re_model::records::{EvidenceRecord, RetrievalError};
use serde_json::{Map, Value};

/// Evidence plus an optional error, like Python's `RetrievalResult`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RetrievalResult {
    /// Retrieved evidence, best first after aggregation.
    pub evidence: Vec<EvidenceRecord>,
    /// Why retrieval failed or came back partial.
    pub error: Option<RetrievalError>,
    /// Rust-only operational data (e.g. `OpenAlex` credits used). Never part
    /// of evidence records, so it cannot affect parity or run-pack hashes.
    pub diagnostics: Map<String, Value>,
}

impl RetrievalResult {
    /// Evidence with no error.
    #[must_use]
    pub fn ok(evidence: Vec<EvidenceRecord>) -> Self {
        Self {
            evidence,
            ..Self::default()
        }
    }

    /// No evidence, with an error.
    #[must_use]
    pub fn failed(error: RetrievalError) -> Self {
        Self {
            error: Some(error),
            ..Self::default()
        }
    }
}
