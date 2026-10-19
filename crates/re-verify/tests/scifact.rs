//! The `SciFact` scoreboard (plan Phase 12): claims become abstract-level
//! pairs and every tier gets one summary row. The lines below are made up
//! in `SciFact`'s format; the dataset itself is downloaded, never
//! committed.

use std::time::Duration;

use re_verify::LexicalOverlap;
use re_verify::bench::{evaluate, scifact_pairs, scoreboard};

const CORPUS: &str = r#"{"doc_id": 11, "title": "Naps", "abstract": ["Daytime naps improve afternoon power.", "The effect was large."], "structured": false}
{"doc_id": 22, "title": "Coffee", "abstract": ["Coffee did not change sprint times."], "structured": false}
"#;

const CLAIMS: &str = r#"{"id": 1, "claim": "Daytime naps improve afternoon power.", "evidence": {"11": [{"sentences": [0], "label": "SUPPORT"}, {"sentences": [1], "label": "SUPPORT"}]}, "cited_doc_ids": [11]}
{"id": 2, "claim": "Coffee improves sprint times.", "evidence": {"22": [{"sentences": [0], "label": "CONTRADICT"}]}, "cited_doc_ids": [22]}

{"id": 3, "claim": "Naps cure jet lag.", "evidence": {}, "cited_doc_ids": [11, 22]}
"#;

#[test]
fn claims_become_one_pair_per_cited_abstract() {
    let pairs = scifact_pairs(CLAIMS, CORPUS).unwrap();
    let summary: Vec<(&str, &str)> = pairs
        .iter()
        .map(|p| (p.id.as_str(), p.label.as_str()))
        .collect();
    assert_eq!(
        summary,
        [
            ("scifact-1-11", "supported"),
            ("scifact-2-22", "contradicted"),
            ("scifact-3-11", "neutral"),
            ("scifact-3-22", "neutral"),
        ]
    );
    assert_eq!(
        pairs[0].evidence,
        "Daytime naps improve afternoon power. The effect was large."
    );
}

#[test]
fn broken_lines_and_missing_abstracts_are_reported() {
    let missing = r#"{"id": 9, "claim": "x", "evidence": {}, "cited_doc_ids": [33]}"#;
    let error = scifact_pairs(missing, CORPUS).unwrap_err();
    assert!(error.contains("cites 33"), "{error}");
    let error = scifact_pairs("not json", CORPUS).unwrap_err();
    assert!(error.starts_with("claims line 1"), "{error}");
}

#[test]
fn the_scoreboard_has_a_row_per_tier() {
    let pairs = scifact_pairs(CLAIMS, CORPUS).unwrap();
    let report = evaluate(&LexicalOverlap, &pairs);
    let table = scoreboard(&[(report, Duration::from_millis(8))]);
    let lines: Vec<&str> = table.lines().collect();
    assert_eq!(
        lines[0],
        "| Tier | Accuracy | F1 supported | F1 contradicted | F1 neutral | Macro F1 | ms / pair |"
    );
    assert_eq!(lines[1], "|---|---:|---:|---:|---:|---:|---:|");
    assert!(lines[2].starts_with("| term_overlap_mvp | "), "{table}");
    assert!(lines[2].ends_with(" 2.0 |"), "{table}");
    assert_eq!(scoreboard(&[]), "");
}
