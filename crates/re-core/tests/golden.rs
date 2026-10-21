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
    assert_eq!(cases.len(), 12);
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

mod task7 {
    use super::{as_str, section};
    use re_core::claims::{ClaimType, DraftClaim, claim_type, extract_draft_claims};
    use re_core::gate::{GateClaim, GateVerification, run_quality_gate};
    use re_core::planning::{
        EvidenceGap, ResearchAngle, evidence_gap_questions, plan_research_angles,
    };
    use re_core::profiles::{classify_research_mode, list_profiles, select_profile};
    use serde_json::Value;

    fn from<T: serde::de::DeserializeOwned>(v: &Value) -> T {
        serde_json::from_value(v.clone()).unwrap_or_else(|e| panic!("{e}: {v}"))
    }

    fn usize_of(v: &Value) -> usize {
        usize::try_from(v.as_u64().expect("unsigned int")).expect("fits usize")
    }

    #[test]
    fn plan_research_angles_matches_python() {
        let cases = section("plan_research_angles");
        assert_eq!(cases.len(), 35);
        for case in cases {
            let query = as_str(&case["query"]);
            let max = usize_of(&case["max_angles"]);
            let expected: Vec<ResearchAngle> = from(&case["expected"]);
            assert_eq!(
                plan_research_angles(query, max).unwrap(),
                expected,
                "query {query:?} max {max} (python stable: {})",
                case["seed_stable"]
            );
        }
    }

    #[test]
    fn evidence_gap_questions_match_python() {
        for case in section("evidence_gap_questions") {
            let claims: Vec<String> = from(&case["claims"]);
            let expected: Vec<EvidenceGap> = from(&case["gaps"]);
            assert_eq!(
                evidence_gap_questions(as_str(&case["query"]), &claims).unwrap(),
                expected
            );
        }
    }

    #[test]
    fn validation_errors_match_python() {
        for case in section("validation_errors") {
            let name = as_str(&case["case"]);
            let actual = match name {
                "angles_empty_query" => evidence_gap_questions("   ", &[]).unwrap_err(),
                "angles_long_query" => evidence_gap_questions(&"x".repeat(8001), &[]).unwrap_err(),
                "gaps_empty_claim" => {
                    evidence_gap_questions("q", &["ok".into(), " ".into()]).unwrap_err()
                }
                "gaps_long_claim" => evidence_gap_questions("q", &["y".repeat(4001)]).unwrap_err(),
                "mode_unknown" => classify_research_mode("q", "bogus").unwrap_err(),
                "profile_unknown" => select_profile("q", "nonexistent").unwrap_err(),
                other => panic!("unhandled validation case {other}"),
            };
            assert_eq!(actual.to_string(), as_str(&case["error"]), "case {name}");
        }
    }

    #[test]
    fn profiles_match_python() {
        let expected = &section("profiles");
        let actual = serde_json::to_value(list_profiles()).unwrap();
        assert_eq!(actual.as_array().unwrap(), *expected);
    }

    #[test]
    fn select_profile_matches_python() {
        let cases = section("select_profile");
        assert_eq!(cases.len(), 20);
        for case in cases {
            let (q, r) = (as_str(&case["query"]), as_str(&case["requested"]));
            assert_eq!(
                select_profile(q, r).unwrap().name,
                as_str(&case["profile"]),
                "{q:?} {r}"
            );
        }
    }

    #[test]
    fn classify_research_mode_matches_python() {
        let cases = section("classify_research_mode");
        assert_eq!(cases.len(), 30);
        for case in cases {
            let (q, m) = (as_str(&case["query"]), as_str(&case["requested"]));
            assert_eq!(
                classify_research_mode(q, m).unwrap().as_str(),
                as_str(&case["mode"]),
                "{q:?} {m}"
            );
        }
    }

    #[test]
    fn claim_type_matches_python() {
        let cases = section("claim_type");
        assert_eq!(cases.len(), 12);
        for case in cases {
            let text = as_str(&case["text"]);
            let expected: ClaimType = from(&case["claim_type"]);
            assert_eq!(claim_type(text), expected, "{text:?}");
        }
    }

    #[test]
    fn extract_claims_matches_python() {
        let cases = section("extract_claims");
        assert_eq!(cases.len(), 4);
        for case in cases {
            let (draft, domain) = (as_str(&case["draft"]), as_str(&case["domain"]));
            let expected: Vec<DraftClaim> = from(&case["claims"]);
            assert_eq!(
                extract_draft_claims(draft, domain),
                expected,
                "{draft:?} {domain}"
            );
        }
    }

    #[test]
    fn quality_gate_matches_python() {
        let cases = section("quality_gate");
        assert_eq!(cases.len(), 6);
        for case in cases {
            let claims: Vec<GateClaim> = from(&case["claims"]);
            let verifications: Vec<GateVerification> = from(&case["verifications"]);
            let gaps = case["gaps"].as_array().unwrap().len();
            let outcome = run_quality_gate("run_test", &claims, &verifications, gaps);
            assert_eq!(
                serde_json::to_value(outcome).unwrap(),
                case["result"],
                "case {}",
                case["name"]
            );
        }
    }
}

mod pyjson {
    use super::{as_str, section};
    use re_core::hashing::stable_json_hash;
    use re_core::pyjson::{DumpOptions, dumps};

    #[test]
    fn dumps_matches_python_byte_for_byte() {
        let cases = section("pyjson");
        assert_eq!(cases.len(), 37);
        for case in cases {
            let v = &case["value"];
            assert_eq!(
                dumps(v, &DumpOptions::sorted()),
                as_str(&case["default"]),
                "{v}"
            );
            assert_eq!(
                dumps(v, &DumpOptions::compact_sorted()),
                as_str(&case["compact"]),
                "{v}"
            );
            assert_eq!(
                dumps(v, &DumpOptions::indented_sorted(2)),
                as_str(&case["indent2"]),
                "{v}"
            );
            assert_eq!(
                stable_json_hash(v),
                as_str(&case["stable_json_hash"]),
                "{v}"
            );
        }
    }
}
