//! Golden parity tests: every vector produced by the Python implementation
//! must be reproduced exactly by the Rust port.
#![allow(clippy::float_cmp, reason = "parity requires bit-exact float equality")]

mod common;

use common::golden;

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
