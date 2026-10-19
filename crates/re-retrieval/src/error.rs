//! Adapter failures (port of `base.AdapterError`).

use re_model::records::RetrievalError;
use serde_json::{Map, Value};

/// A failure inside one adapter, mapped to a [`RetrievalError`] for callers.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("{error_type}: {message}")]
pub struct AdapterError {
    /// One of `re_model::records::RETRIEVAL_ERROR_TYPES`.
    pub error_type: &'static str,
    /// Human-readable detail. Never contains credentials.
    pub message: String,
    /// Whether a retry may succeed.
    pub retryable: bool,
    /// Extra context, e.g. `retry_after_secs` for HTTP 429.
    pub extra: Map<String, Value>,
}

impl AdapterError {
    /// Build an error with no extra context.
    #[must_use]
    pub fn new(error_type: &'static str, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            error_type,
            message: message.into(),
            retryable,
            extra: Map::new(),
        }
    }

    /// Convert to the public record, tagging it with the source name like
    /// Python's `exc.to_retrieval_error({"source": name})`.
    #[must_use]
    pub fn to_retrieval_error(&self, source: &str) -> RetrievalError {
        let mut metadata = Map::new();
        metadata.insert("source".into(), source.into());
        for (key, value) in &self.extra {
            metadata.insert(key.clone(), value.clone());
        }
        RetrievalError {
            error_type: self.error_type.to_string(),
            message: self.message.clone(),
            retryable: self.retryable,
            metadata,
        }
    }
}
