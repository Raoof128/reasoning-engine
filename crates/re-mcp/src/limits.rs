//! Input limits, identical to `validation.py` and the inline limits in
//! `server.py`.

pub use re_core::validation::{
    MAX_ANGLES, MAX_CRITIQUE_CHARS, MAX_LEARNING_CHARS, MAX_LEARNINGS, MAX_MEMORY_LIMIT,
    MAX_QUERY_CHARS, MAX_QUESTION_CHARS, MAX_SOURCE_FIELD_CHARS, MAX_SOURCES, MAX_TAG_CHARS,
    MAX_TAGS, MAX_TOP_K, MAX_TRACE_STEP_CHARS, MAX_TRACE_STEPS,
};

/// `run_id` length cap (`validate_text(run_id, "run_id", 128)`).
pub const MAX_RUN_ID_CHARS: usize = 128;
/// `scholar_search_tool` limit cap.
pub const MAX_SEARCH_LIMIT: i64 = 25;
/// `sanitize_content` input cap (`MAX_QUERY_CHARS * 4`).
pub const MAX_RAW_TEXT_CHARS: usize = MAX_QUERY_CHARS * 4;
/// Pipeline draft cap (`MAX_QUERY_CHARS * 4`).
pub const MAX_DRAFT_CHARS: usize = MAX_QUERY_CHARS * 4;
