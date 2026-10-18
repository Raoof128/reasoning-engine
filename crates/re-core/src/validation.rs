//! Input limits and text validation (port of the pure parts of validation.py).
//!
//! The MCP layer re-exports these so both layers share one set of limits and
//! the same error messages as Python.

use crate::types::CoreError;

/// Maximum characters in a research query.
pub const MAX_QUERY_CHARS: usize = 8000;
/// Maximum steps in a branch trace.
pub const MAX_TRACE_STEPS: usize = 80;
/// Maximum characters per trace step or claim.
pub const MAX_TRACE_STEP_CHARS: usize = 4000;
/// Maximum sources per branch.
pub const MAX_SOURCES: usize = 50;
/// Maximum characters per source field.
pub const MAX_SOURCE_FIELD_CHARS: usize = 4000;
/// Maximum characters in a critique.
pub const MAX_CRITIQUE_CHARS: usize = 8000;
/// Maximum learnings saved per memory.
pub const MAX_LEARNINGS: usize = 30;
/// Maximum characters per learning.
pub const MAX_LEARNING_CHARS: usize = 2000;
/// Maximum domain tags per memory.
pub const MAX_TAGS: usize = 20;
/// Maximum characters per tag.
pub const MAX_TAG_CHARS: usize = 80;
/// Maximum memories returned by recall.
pub const MAX_MEMORY_LIMIT: usize = 20;
/// Maximum consensus candidates.
pub const MAX_TOP_K: usize = 10;
/// Maximum research angles.
pub const MAX_ANGLES: usize = 12;
/// Maximum characters per generated question.
pub const MAX_QUESTION_CHARS: usize = 500;

/// Trim `value` and enforce non-emptiness and a character limit.
///
/// Mirrors Python's `validate_text`: the limit counts characters (code
/// points), not bytes, and applies to the trimmed text.
///
/// # Errors
/// [`CoreError::EmptyText`] when nothing is left after trimming, or
/// [`CoreError::TextTooLong`] when the trimmed text is over `max_chars`.
pub fn validate_text(value: &str, field: &str, max_chars: usize) -> Result<String, CoreError> {
    let cleaned = value.trim();
    if cleaned.is_empty() {
        return Err(CoreError::EmptyText {
            field: field.to_string(),
        });
    }
    if cleaned.chars().count() > max_chars {
        return Err(CoreError::TextTooLong {
            field: field.to_string(),
            max_chars,
        });
    }
    Ok(cleaned.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_and_counts_characters_not_bytes() {
        assert_eq!(validate_text("  hi  ", "q", 2).unwrap(), "hi");
        assert!(validate_text("é".repeat(3).as_str(), "q", 3).is_ok());
        assert_eq!(
            validate_text("abcd", "query", 3).unwrap_err().to_string(),
            "query exceeds 3 characters"
        );
        assert_eq!(
            validate_text(" \t", "claims[1]", 3)
                .unwrap_err()
                .to_string(),
            "claims[1] must not be empty"
        );
    }
}
