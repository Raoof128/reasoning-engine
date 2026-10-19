//! Claim extraction and lexical verification agree with Python.

mod common;

use std::ffi::OsStr;

use common::verify_tool;
use re_model::records::EvidenceRecord;
use re_retrieval::base::{RecordInput, build_record};
use re_verify::claims::extract_claims;
use re_verify::{LexicalOverlap, Verifier};
use serde_json::{Value, json};

const DRAFT: &str = "Reward models improve accuracy by 12%. Process rewards may help planning! \
    Should teams adopt them? Smoking causes cancer because of tar.   Café ünïcode reasoning works well. \
    Verification beats sampling in practice. Unrelated statement about gardening tomatoes. x.";

fn evidence() -> Vec<EvidenceRecord> {
    let snippets = [
        "Process reward models improve reasoning accuracy across benchmarks.",
        "Reward models improve accuracy.",
        "verification beats sampling in practice for verification tasks",
        "Smoking causes lung cancer; tar is a factor.",
        "Verification beats sampling in practice.",
    ];
    snippets
        .iter()
        .enumerate()
        .map(|(i, snippet)| {
            build_record(RecordInput {
                title: format!("E{i}"),
                snippet: (*snippet).into(),
                ..RecordInput::new("run_x", "openalex", "q", i + 1)
            })
        })
        .collect()
}

fn compare(domain: &str, evidence: &[EvidenceRecord]) {
    let mut claims = extract_claims("run_x", DRAFT, domain).expect("claims");
    let verifications = LexicalOverlap.verify(&mut claims, evidence);

    let dir = tempfile::tempdir().expect("tmp");
    let input = dir.path().join("claims.json");
    let items: Vec<Value> = evidence
        .iter()
        .map(|e| json!({"evidence_id": e.evidence_id, "snippet": e.snippet}))
        .collect();
    let payload = json!({"run_id": "run_x", "draft": DRAFT, "domain": domain, "evidence": items});
    std::fs::write(&input, serde_json::to_vec(&payload).expect("json")).expect("write");
    let python = verify_tool(&[OsStr::new("claims"), input.as_os_str()]);

    let ours: Vec<Value> = claims
        .iter()
        .map(|c| serde_json::to_value(c).expect("json"))
        .collect();
    assert_eq!(Value::Array(ours), python["claims"], "claims for {domain}");
    let ours: Vec<Value> = verifications
        .iter()
        .map(|v| {
            let mut v = serde_json::to_value(v).expect("json");
            let map = v.as_object_mut().expect("object");
            assert!(
                map.remove("verification_id")
                    .expect("id")
                    .as_str()
                    .expect("str")
                    .starts_with("ver_")
            );
            map.remove("verified_at");
            v
        })
        .collect();
    assert_eq!(
        Value::Array(ours),
        python["verifications"],
        "verifications for {domain}"
    );
}

#[test]
fn matches_python_with_evidence() {
    compare("general", &evidence());
}

#[test]
fn matches_python_in_risky_domains_and_without_evidence() {
    compare("medicine", &evidence());
    compare("security", &[]);
}

#[test]
fn ties_go_to_the_first_evidence_item() {
    let mut claims = extract_claims(
        "run_x",
        "Verification beats sampling in practice.",
        "general",
    )
    .expect("claims");
    let verifications = LexicalOverlap.verify(&mut claims, &evidence());
    // Snippets 3 and 5 both score 1.0; Python's stable descending sort keeps 3.
    assert_eq!(verifications[0].evidence_ids, ["ev_openalex_0003"]);
    assert_eq!(claims[0].status, "supported");
    assert_eq!(claims[0].confidence, Some(1.0));
}
