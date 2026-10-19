//! Run packs are byte-identical across implementations and each side
//! verifies the other's packs.

mod common;

use std::ffi::OsStr;
use std::fs;
use std::path::Path;

use common::verify_tool;
use re_model::records::ResearchRun;
use re_verify::runpack::{
    ARTIFACTS, RunPackInput, export_run_pack, source_bib, verify_run_pack_attestation,
};
use serde_json::{Value, json};

struct Inputs {
    run: ResearchRun,
    evidence: Vec<Value>,
    gaps: Vec<Value>,
    claims: Vec<Value>,
    verifications: Vec<Value>,
    provenance: Vec<Value>,
    gate: Value,
    report: String,
}

/// Inputs chosen to stress the JSON writer: non-ASCII, astral characters,
/// floats Python prints in exponent form, nulls, nesting, empty containers.
fn inputs() -> Inputs {
    let run = ResearchRun {
        run_id: "run_0123456789ab".into(),
        query: "اثر مدل پاداش on café reasoning 🦀".into(),
        profile: "general".into(),
        mode: "standard".into(),
        created_at: "2026-10-19T01:02:03Z".into(),
        status: "active".into(),
    };
    let evidence = vec![
        json!({"evidence_id": "ev_0001", "title": "Reward “models” — ünïcode", "year": 2024,
               "doi": "10.1/x", "score": 1e-05, "metadata": {"z": [1, 2.5, 1e16], "a": null, "also_found_in": []},
               "snippet": "Line one\nline two\ttab \u{1F980}"}),
        json!({"evidence_id": "ev_0002", "title": "No year", "year": null, "doi": null, "score": 0.1}),
        json!({"evidence_id": "ev_0003", "title": "Zero year", "year": 0, "doi": ""}),
    ];
    let gaps = vec![
        json!({"gap_id": "gap_1", "reason": "No evidence found."}),
        json!({"gap_id": "gap_2"}),
    ];
    let claims = vec![
        json!({"claim_id": "claim_0001", "text": "Rewards help", "evidence_ids": ["ev_0001"], "confidence": 0.67}),
        json!({"claim_id": "claim_0002", "text": "No ids"}),
    ];
    let verifications =
        vec![json!({"verification_id": "ver_1", "claim_id": "claim_0001", "confidence": 0.5})];
    let provenance = vec![
        json!({"run_id": "run_0123456789ab", "event_type": "run_created", "timestamp": "2026-10-19T01:02:03Z", "query": "é"}),
        json!({"event_type": "scholar_search_requested", "limit": 10}),
    ];
    let gate = json!({"gate_id": "gate_run", "result": "pass_with_warnings", "warnings": ["w"], "blocking_failures": []});
    let report = "# Research Report run_0123456789ab\n\n- [supported] Rewards help ✓\n".to_string();
    Inputs {
        run,
        evidence,
        gaps,
        claims,
        verifications,
        provenance,
        gate,
        report,
    }
}

fn export_rust(base: &Path, i: &Inputs) -> std::path::PathBuf {
    export_run_pack(
        base,
        &RunPackInput {
            run: &i.run,
            evidence: &i.evidence,
            gaps: &i.gaps,
            claims: &i.claims,
            verifications: &i.verifications,
            provenance: &i.provenance,
            quality_gate: &i.gate,
            report_markdown: &i.report,
            verifier_method: "term_overlap_mvp",
            verifier_models: &[],
            replay: None,
            signer: None,
            references: None,
        },
    )
    .expect("rust export")
}

fn export_python(base: &Path, i: &Inputs) -> std::path::PathBuf {
    let input = base.join("input.json");
    let payload = json!({
        "run": i.run, "evidence": i.evidence, "gaps": i.gaps, "claims": i.claims,
        "verifications": i.verifications, "provenance": i.provenance,
        "quality_gate": i.gate, "report_markdown": i.report,
    });
    fs::write(&input, serde_json::to_vec(&payload).expect("json")).expect("write input");
    let out = verify_tool(&[OsStr::new("export"), input.as_os_str(), base.as_os_str()]);
    out["run_pack"].as_str().expect("path").into()
}

/// Python's `write_text` translates `\n` to `\r\n` on Windows; compare the
/// text Python meant to write.
fn python_bytes(bytes: Vec<u8>) -> Vec<u8> {
    if cfg!(windows) {
        String::from_utf8(bytes)
            .expect("utf-8")
            .replace("\r\n", "\n")
            .into_bytes()
    } else {
        bytes
    }
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).expect("read")).expect("json")
}

#[test]
fn artifacts_are_byte_identical_to_python() {
    let (rust_dir, py_dir) = (
        tempfile::tempdir().expect("tmp"),
        tempfile::tempdir().expect("tmp"),
    );
    let i = inputs();
    let rust = export_rust(rust_dir.path(), &i);
    let python = export_python(py_dir.path(), &i);
    for name in ARTIFACTS {
        let ours = fs::read(rust.join(name)).expect("rust artifact");
        let theirs = python_bytes(fs::read(python.join(name)).expect("python artifact"));
        assert_eq!(
            String::from_utf8_lossy(&ours),
            String::from_utf8_lossy(&theirs),
            "{name}"
        );
    }
    if !cfg!(windows) {
        let (ours, theirs) = (
            read_json(&rust.join("attestation.json")),
            read_json(&python.join("attestation.json")),
        );
        assert_eq!(ours["artifact_hashes"], theirs["artifact_hashes"]);
        assert_eq!(ours["run_pack_hash"], theirs["run_pack_hash"]);
    }
}

#[test]
fn each_side_verifies_the_others_pack() {
    let (rust_dir, py_dir) = (
        tempfile::tempdir().expect("tmp"),
        tempfile::tempdir().expect("tmp"),
    );
    let i = inputs();
    let rust = export_rust(rust_dir.path(), &i);
    let python = export_python(py_dir.path(), &i);

    let by_python = verify_tool(&[OsStr::new("verify"), rust.as_os_str()]);
    assert_eq!(by_python["valid"], true, "{by_python}");
    let by_rust = verify_run_pack_attestation(&python);
    assert_eq!(by_rust["valid"], true, "{by_rust}");
    assert_eq!(
        by_rust["run_pack_hash"],
        read_json(&python.join("attestation.json"))["run_pack_hash"]
    );
}

#[test]
fn tampering_is_detected() {
    let dir = tempfile::tempdir().expect("tmp");
    let pack = export_rust(dir.path(), &inputs());
    assert_eq!(verify_run_pack_attestation(&pack)["valid"], true);

    fs::write(pack.join("report.md"), "edited\n").expect("tamper");
    let result = verify_run_pack_attestation(&pack);
    assert_eq!(result["valid"], false);
    assert_eq!(result["mismatched_artifacts"], json!(["report.md"]));

    fs::remove_file(pack.join("sources.bib")).expect("remove");
    assert_eq!(
        verify_run_pack_attestation(&pack)["missing_artifacts"],
        json!(["sources.bib"])
    );

    fs::remove_file(pack.join("attestation.json")).expect("remove");
    let gone = verify_run_pack_attestation(&pack);
    assert_eq!(
        gone,
        json!({"valid": false, "mismatched_artifacts": ["attestation.json"], "missing_artifacts": ["attestation.json"]})
    );
}

#[test]
fn rewritten_attestations_and_path_tricks_are_rejected() {
    let dir = tempfile::tempdir().expect("tmp");
    let pack = export_rust(dir.path(), &inputs());
    let path = pack.join("attestation.json");
    let original = read_json(&path);

    // Edit an artifact and its listed hash: Python reads this as valid
    // because it never rechecks run_pack_hash; the port does.
    fs::write(pack.join("report.md"), "forged\n").expect("forge");
    let mut forged = original.clone();
    forged["artifact_hashes"]["report.md"] = json!(re_core::hashing::sha256_prefixed(b"forged\n"));
    fs::write(&path, serde_json::to_vec(&forged).expect("json")).expect("write");
    let result = verify_run_pack_attestation(&pack);
    assert_eq!(result["valid"], false);
    assert_eq!(result["mismatched_artifacts"], json!(["attestation.json"]));

    let mut escaping = original;
    escaping["artifact_hashes"] = json!({"../outside": "sha256:00"});
    fs::write(&path, serde_json::to_vec(&escaping).expect("json")).expect("write");
    assert_eq!(
        verify_run_pack_attestation(&pack)["mismatched_artifacts"],
        json!(["../outside", "attestation.json"])
    );
}

#[test]
fn bib_entries_follow_python_formatting() {
    let bib = source_bib(&inputs().evidence);
    assert!(bib.starts_with("@article{ev_0001,\n  title = {Reward “models” — ünïcode},\n  year = {2024},\n  doi = {10.1/x}\n}"));
    assert!(
        bib.contains("@article{ev_0003,\n  title = {Zero year},\n  year = {},\n  doi = {}\n}\n")
    );
    assert_eq!(source_bib(&[]), "");
}
