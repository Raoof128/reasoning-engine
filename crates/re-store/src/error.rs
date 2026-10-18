//! Store errors. `Invalid` carries the exact message Python's `ValueError`
//! would, because MCP clients see these strings.

/// Errors from the store.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// A lookup or validation failure, with Python's message.
    #[error("{0}")]
    Invalid(String),
    /// A `SQLite` failure.
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// A filesystem failure while preparing the database path.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    /// A stored JSON column could not be parsed.
    #[error("invalid JSON in {column}: {source}")]
    Json {
        /// Column name.
        column: &'static str,
        /// Parser error.
        source: serde_json::Error,
    },
}

impl StoreError {
    pub(crate) fn invalid(message: impl Into<String>) -> Self {
        Self::Invalid(message.into())
    }
}
