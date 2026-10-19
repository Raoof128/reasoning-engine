//! Golden vectors from `tests/golden/golden_vectors.json` for the retrieval
//! helpers.

use std::path::PathBuf;

use re_retrieval::base::{normalize_doi, relevance_score};
use serde_json::Value;

fn section(name: &str) -> Vec<Value> {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/golden/golden_vectors.json");
    let golden: Value = serde_json::from_str(&std::fs::read_to_string(path).expect("golden file"))
        .expect("valid JSON");
    golden[name].as_array().cloned().expect("section")
}

fn small(value: &Value) -> usize {
    usize::try_from(value.as_u64().expect("int")).expect("small")
}

#[test]
fn relevance_score_matches_python() {
    let cases = section("relevance_score");
    assert_eq!(cases.len(), 5);
    for case in cases {
        let score = relevance_score(
            small(&case["position"]),
            small(&case["limit"]),
            case["citations"].as_f64(),
        );
        assert_eq!(
            score.to_bits(),
            case["score"].as_f64().expect("float").to_bits(),
            "{case}"
        );
    }
}

#[test]
fn normalize_doi_matches_python() {
    let cases = section("normalize_doi");
    assert_eq!(cases.len(), 5);
    for case in cases {
        assert_eq!(
            normalize_doi(&case["doi"]),
            case["normalized"].as_str().map(str::to_string),
            "{case}"
        );
    }
}
