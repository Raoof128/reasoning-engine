//! Run-pack export and tamper-evident attestation (port of `runpack.py`).
//!
//! Every artifact is byte-identical to Python's on the same inputs (JSON via
//! `re_core::pyjson`), so packs written by either implementation verify with
//! the other. Two deliberate differences, both stricter:
//!
//! - Files are always written with `\n` line endings. Python's `write_text`
//!   writes `\r\n` on Windows, so its Windows packs hash differently.
//! - Verification refuses artifact names that are not plain file names and
//!   checks `run_pack_hash` against the listed hashes, so a pack whose
//!   attestation was edited along with an artifact no longer reads valid.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use re_core::hashing::{sha256_prefixed, stable_json_hash};
use re_core::pyjson::{DumpOptions, dumps};
use re_model::clock::utc_now;
use re_model::records::ResearchRun;
use serde_json::{Map, Value, json};

/// Hashed artifacts, in attestation order.
pub const ARTIFACTS: [&str; 12] = [
    "run.json",
    "provenance.jsonl",
    "evidence_ledger.json",
    "evidence_gaps.json",
    "claims.json",
    "claim_evidence_links.json",
    "claim_verifications.json",
    "quality_gate.json",
    "report.md",
    "unresolved_gaps.md",
    "sources.bib",
    "methodology.md",
];

/// The fixed methodology statement.
pub const METHODOLOGY: &str = "# Methodology\n\n\
Evidence was retrieved through configured adapters, claims were verified against the same \
evidence records exported in the run pack, and the quality gate was run before export.\n\n\
MVP verification uses deterministic lexical overlap as a placeholder verifier. It is \
suitable for pipeline testing, not final semantic claim verification.\n";

/// Everything a run pack contains, as JSON payloads.
#[derive(Debug, Clone, Copy)]
pub struct RunPackInput<'a> {
    /// The run.
    pub run: &'a ResearchRun,
    /// Evidence `to_dict()` payloads.
    pub evidence: &'a [Value],
    /// Gap payloads.
    pub gaps: &'a [Value],
    /// Claim payloads.
    pub claims: &'a [Value],
    /// Verification payloads.
    pub verifications: &'a [Value],
    /// Provenance events.
    pub provenance: &'a [Value],
    /// Quality-gate result.
    pub quality_gate: &'a Value,
    /// The Markdown report.
    pub report_markdown: &'a str,
}

/// `json.dumps(payload, indent=2, sort_keys=True) + "\n"`.
fn pretty(payload: &Value) -> String {
    let mut text = dumps(payload, &DumpOptions::indented_sorted(2));
    text.push('\n');
    text
}

/// Python's `f"{value}"` for the scalar fields a bib entry uses.
fn fmt(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => re_retrieval::base::py_str(other),
    }
}

/// `x or ""`, then formatted.
fn fmt_or_empty(value: Option<&Value>) -> String {
    match value {
        Some(v) if re_retrieval::base::truthy(v) => fmt(Some(v)),
        _ => String::new(),
    }
}

/// `sources.bib`: one `@article` per evidence item.
#[must_use]
pub fn source_bib(evidence: &[Value]) -> String {
    let entries: Vec<String> = evidence
        .iter()
        .map(|item| {
            format!(
                "@article{{{},\n  title = {{{}}},\n  year = {{{}}},\n  doi = {{{}}}\n}}",
                fmt(item.get("evidence_id")),
                fmt(item.get("title")),
                fmt_or_empty(item.get("year")),
                fmt_or_empty(item.get("doi")),
            )
        })
        .collect();
    let mut out = entries.join("\n\n");
    if !entries.is_empty() {
        out.push('\n');
    }
    out
}

fn unresolved_gaps(gaps: &[Value]) -> String {
    let lines: Vec<String> = gaps
        .iter()
        .map(|gap| {
            let reason = gap
                .get("reason")
                .map_or_else(|| "Unresolved evidence gap".into(), |r| fmt(Some(r)));
            format!("- {reason}")
        })
        .collect();
    let mut out = lines.join("\n");
    if !lines.is_empty() {
        out.push('\n');
    }
    out
}

fn hash_file(path: &Path) -> io::Result<String> {
    Ok(sha256_prefixed(&fs::read(path)?))
}

/// Write the pack to `<base_dir>/<run_id>/` and attest it.
///
/// # Errors
/// Filesystem failures.
pub fn export_run_pack(base_dir: &Path, input: &RunPackInput<'_>) -> io::Result<PathBuf> {
    let output = base_dir.join(&input.run.run_id);
    fs::create_dir_all(&output)?;
    let write = |name: &str, text: &str| fs::write(output.join(name), text);

    let run = serde_json::to_value(input.run).map_err(io::Error::other)?;
    write("run.json", &pretty(&run))?;
    let provenance: String = input
        .provenance
        .iter()
        .map(|event| dumps(event, &DumpOptions::sorted()) + "\n")
        .collect();
    write("provenance.jsonl", &provenance)?;
    write(
        "evidence_ledger.json",
        &pretty(&Value::Array(input.evidence.to_vec())),
    )?;
    write(
        "evidence_gaps.json",
        &pretty(&Value::Array(input.gaps.to_vec())),
    )?;
    write("claims.json", &pretty(&Value::Array(input.claims.to_vec())))?;
    let links: Vec<Value> = input
        .claims
        .iter()
        .map(|claim| {
            json!({
                "claim_id": claim.get("claim_id").cloned().unwrap_or(Value::Null),
                "evidence_ids": claim.get("evidence_ids").cloned().unwrap_or_else(|| json!([])),
            })
        })
        .collect();
    write("claim_evidence_links.json", &pretty(&Value::Array(links)))?;
    write(
        "claim_verifications.json",
        &pretty(&Value::Array(input.verifications.to_vec())),
    )?;
    write("quality_gate.json", &pretty(input.quality_gate))?;
    write("report.md", input.report_markdown)?;
    write("unresolved_gaps.md", &unresolved_gaps(input.gaps))?;
    write("sources.bib", &source_bib(input.evidence))?;
    write("methodology.md", METHODOLOGY)?;

    let mut artifact_hashes = Map::new();
    for name in ARTIFACTS {
        artifact_hashes.insert(name.into(), hash_file(&output.join(name))?.into());
    }
    let artifact_hashes = Value::Object(artifact_hashes);
    let attestation = json!({
        "run_id": input.run.run_id,
        "created_at": utc_now(),
        "engine_version": env!("CARGO_PKG_VERSION"),
        "run_pack_hash": stable_json_hash(&artifact_hashes),
        "artifact_hashes": artifact_hashes,
        "signature": null,
        "signature_algorithm": null,
    });
    write("attestation.json", &pretty(&attestation))?;
    Ok(output)
}

/// A plain file name: no separators, no `..`, nothing absolute.
fn is_plain_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', ':'])
}

/// Recompute every artifact hash listed in `attestation.json`.
///
/// Returns `{valid, mismatched_artifacts, missing_artifacts, run_pack_hash}`
/// like Python (without `run_pack_hash` when there is no attestation).
#[must_use]
pub fn verify_run_pack_attestation(run_pack_dir: &Path) -> Value {
    let attestation_path = run_pack_dir.join("attestation.json");
    let unreadable = || {
        json!({
            "valid": false,
            "mismatched_artifacts": ["attestation.json"],
            "missing_artifacts": ["attestation.json"],
        })
    };
    let Ok(text) = fs::read_to_string(&attestation_path) else {
        return unreadable();
    };
    let Ok(attestation) = serde_json::from_str::<Value>(&text) else {
        return json!({
            "valid": false,
            "mismatched_artifacts": ["attestation.json"],
            "missing_artifacts": [],
        });
    };
    let empty = Map::new();
    let hashes = attestation
        .get("artifact_hashes")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    let mut mismatched = Vec::new();
    let mut missing = Vec::new();
    for (name, expected) in hashes {
        let artifact = run_pack_dir.join(name);
        if !is_plain_name(name) {
            mismatched.push(name.clone());
        } else if !artifact.exists() {
            missing.push(name.clone());
        } else if hash_file(&artifact).ok().as_deref() != expected.as_str() {
            mismatched.push(name.clone());
        }
    }
    let run_pack_hash = attestation
        .get("run_pack_hash")
        .cloned()
        .unwrap_or(Value::Null);
    let listed = Value::Object(hashes.clone());
    let summary_ok = run_pack_hash.as_str() == Some(stable_json_hash(&listed).as_str());
    if !summary_ok && !mismatched.iter().any(|n| n == "attestation.json") {
        mismatched.push("attestation.json".into());
    }
    json!({
        "valid": mismatched.is_empty() && missing.is_empty() && !hashes.is_empty(),
        "mismatched_artifacts": mismatched,
        "missing_artifacts": missing,
        "run_pack_hash": run_pack_hash,
    })
}
