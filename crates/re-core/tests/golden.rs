//! Golden parity tests: every vector produced by the Python implementation
//! must be reproduced exactly by the Rust port.
#![allow(clippy::float_cmp, reason = "parity requires bit-exact float equality")]

mod common;

use common::{golden, section};
use re_core::difficulty::estimate_difficulty;

fn as_f64(v: &serde_json::Value) -> f64 {
    v.as_f64()
        .unwrap_or_else(|| panic!("expected number, got {v}"))
}

fn as_str(v: &serde_json::Value) -> &str {
    v.as_str()
        .unwrap_or_else(|| panic!("expected string, got {v}"))
}

#[test]
fn difficulty_matches_python() {
    for case in section("difficulty") {
        let query = as_str(&case["query"]);
        let expected = as_f64(&case["difficulty"]);
        assert_eq!(estimate_difficulty(query), expected, "query: {query:?}");
    }
}

#[test]
fn golden_file_has_expected_sections() {
    let keys: Vec<&str> = golden()
        .as_object()
        .expect("golden root is an object")
        .keys()
        .map(String::as_str)
        .collect();
    for expected in [
        "allocate_budget",
        "difficulty",
        "normalize_doi",
        "relevance_score",
        "sanitize_content",
        "select_best_ucb",
        "select_branches",
        "ucb_score",
        "ucb_unvisited_is_infinite",
    ] {
        assert!(
            keys.contains(&expected),
            "missing golden section {expected}"
        );
    }
}
