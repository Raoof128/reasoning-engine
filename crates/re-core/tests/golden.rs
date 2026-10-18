//! Golden parity tests: every vector produced by the Python implementation
//! must be reproduced exactly by the Rust port.
#![allow(clippy::float_cmp, reason = "parity requires bit-exact float equality")]

mod common;

use common::{golden, section};
use indexmap::IndexMap;
use re_core::difficulty::estimate_difficulty;
use re_core::dora::{AllocationResult, BudgetAllocation, allocate_budget, select_branches};
use re_core::sanitize::sanitize_content;
use re_core::ucb::{BranchStats, select_best_ucb, ucb_score};

#[test]
fn sanitize_content_matches_python() {
    let cases = section("sanitize_content");
    assert_eq!(cases.len(), 8);
    for case in cases {
        let raw = as_str(&case["raw"]);
        let expected = as_str(&case["clean"]);
        assert_eq!(sanitize_content(raw), expected, "raw {raw:?}");
    }
}

#[test]
fn ucb_score_matches_python() {
    let cases = section("ucb_score");
    assert_eq!(cases.len(), 4);
    for case in cases {
        let q = as_f64(&case["q"]);
        let parent = case["parent_visits"].as_u64().expect("int");
        let node = case["node_visits"].as_u64().expect("int");
        let expected = as_f64(&case["score"]);
        assert_eq!(ucb_score(q, parent, node), expected, "{case}");
    }
}

#[test]
fn ucb_unvisited_is_infinite_matches_python() {
    assert_eq!(
        golden()["ucb_unvisited_is_infinite"],
        serde_json::Value::Bool(true)
    );
    assert_eq!(ucb_score(0.5, 10, 0), f64::INFINITY);
}

#[test]
fn select_best_ucb_matches_python() {
    let cases = section("select_best_ucb");
    assert_eq!(cases.len(), 3);
    for case in cases {
        let branches: IndexMap<String, BranchStats> =
            serde_json::from_value(case["branches"].clone()).expect("branches parse");
        let total = case["total_visits"].as_u64().expect("int");
        let expected = case["best"].as_str().map(str::to_string);
        assert_eq!(select_best_ucb(&branches, total), expected, "{case}");
    }
}

#[test]
fn allocate_budget_matches_python() {
    let cases = section("allocate_budget");
    assert_eq!(cases.len(), 10);
    for case in cases {
        let difficulty = as_f64(&case["difficulty"]);
        let expected: BudgetAllocation =
            serde_json::from_value(case["budget"].clone()).expect("budget parses");
        assert_eq!(
            allocate_budget(difficulty),
            expected,
            "difficulty {difficulty}"
        );
    }
}

#[test]
fn select_branches_matches_python() {
    let cases = section("select_branches");
    assert_eq!(cases.len(), 30);
    for case in cases {
        let scores: IndexMap<String, f64> =
            serde_json::from_value(case["scores"].clone()).expect("scores parse");
        let budget = case["budget_remaining"].as_i64().expect("budget int");
        let expected: AllocationResult =
            serde_json::from_value(case["result"].clone()).expect("result parses");
        assert_eq!(
            select_branches(&scores, budget),
            expected,
            "scores {scores:?} budget {budget}"
        );
    }
}

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
