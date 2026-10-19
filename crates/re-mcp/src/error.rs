//! Tool failures. Python's `FastMCP` turns any exception into a result with
//! `isError: true` and the text `Error executing tool <name>: <message>`;
//! the port does the same, so clients and models see identical errors.

/// A tool failure carrying Python's message.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct ToolError(pub String);

impl ToolError {
    /// An error with `message`.
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl From<re_store::StoreError> for ToolError {
    fn from(error: re_store::StoreError) -> Self {
        Self(error.to_string())
    }
}

impl From<re_verify::VerifyError> for ToolError {
    fn from(error: re_verify::VerifyError) -> Self {
        Self(error.to_string())
    }
}

impl From<re_core::types::CoreError> for ToolError {
    fn from(error: re_core::types::CoreError) -> Self {
        Self(error.to_string())
    }
}
