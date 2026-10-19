//! Errors from the verification pipeline. Each wraps the layer's own error,
//! whose message matches what Python raises.

/// Anything that can stop a pipeline step.
#[derive(Debug, thiserror::Error)]
pub enum VerifyError {
    /// Unknown profile or mode.
    #[error(transparent)]
    Core(#[from] re_core::types::CoreError),
    /// An invalid record.
    #[error(transparent)]
    Model(#[from] re_model::records::ModelError),
    /// A database failure.
    #[error(transparent)]
    Store(#[from] re_store::StoreError),
    /// The search backend could not be built.
    #[error(transparent)]
    Retrieval(#[from] re_retrieval::AdapterError),
    /// Writing the run pack failed.
    #[error("run pack io error: {0}")]
    Io(#[from] std::io::Error),
    /// `REASONING_ENGINE_VERIFIER` names no known verifier.
    #[error("unknown verifier: {0} (expected one of: lexical)")]
    UnknownVerifier(String),
}
