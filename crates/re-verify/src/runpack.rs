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

use std::collections::BTreeMap;
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
    /// The verifier's method, e.g. `term_overlap_mvp`.
    pub verifier_method: &'a str,
    /// Pinned models the verifier used; empty for the lexical tier.
    pub verifier_models: &'a [String],
    /// What a recorded run keeps for `replay`, if it was recorded.
    pub replay: Option<&'a ReplayFiles>,
    /// Signs `run_pack_hash`, if signing is configured.
    pub signer: Option<&'a dyn AttestationSigner>,
    /// `(file name, Markdown)` of the styled reference list, if rendered.
    /// It is hashed with the core artifacts.
    pub references: Option<(&'a str, &'a str)>,
}

/// Something that signs a run pack's `run_pack_hash` (plan Phase 7). The
/// Ed25519 implementation lives in `signing` (`trust` feature);
/// without a signer the attestation keeps Python's `null` signature.
pub trait AttestationSigner: Send + Sync {
    /// `signature_algorithm`, e.g. `ed25519`.
    fn algorithm(&self) -> &'static str;
    /// The public key, base64.
    fn public_key(&self) -> String;
    /// The signature over `message`, base64.
    fn sign(&self, message: &[u8]) -> String;
}

impl std::fmt::Debug for dyn AttestationSigner + '_ {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} signer {}", self.algorithm(), self.public_key())
    }
}

/// The extra files of a recorded run (plan Phase 7): `replay.json` holds
/// the inputs, `http/<key>.json` the recorded responses. They are hashed
/// under the attestation's `replay` key, outside `artifact_hashes`, so
/// Python's verifier still checks the core artifacts unchanged.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ReplayFiles {
    /// Query, draft, mode, profile, clock, seed, sources and verifier.
    pub inputs: Value,
    /// `<key>.json` to file text.
    pub recordings: BTreeMap<String, String>,
}

/// The replay inputs file.
pub const REPLAY_INPUTS: &str = "replay.json";
/// Where recorded responses live inside a pack.
pub const RECORDINGS_DIR: &str = "http";

/// `methodology.md`: Python's fixed text for model-free runs (keeping those
/// packs byte-identical), plus a section naming the method and pinned
/// models when a model tier verified the claims.
#[must_use]
pub fn methodology(method: &str, models: &[String]) -> String {
    if models.is_empty() {
        return METHODOLOGY.to_string();
    }
    let mut text = format!(
        "{METHODOLOGY}\n## Verifier\n\nClaims were verified with `{method}` instead of lexical overlap.\n\n"
    );
    for model in models {
        text.push_str("- `");
        text.push_str(model);
        text.push_str("` (pinned Hugging Face revision, SHA-256 checked on load)\n");
    }
    text
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
    write(
        "methodology.md",
        &methodology(input.verifier_method, input.verifier_models),
    )?;

    let mut artifact_hashes = Map::new();
    for name in ARTIFACTS {
        artifact_hashes.insert(name.into(), hash_file(&output.join(name))?.into());
    }
    if let Some((name, text)) = input.references {
        if !is_plain_name(name) {
            return Err(io::Error::other(format!(
                "bad references file name: {name}"
            )));
        }
        write(name, text)?;
        artifact_hashes.insert(name.into(), hash_file(&output.join(name))?.into());
    }
    let artifact_hashes = Value::Object(artifact_hashes);
    let mut attestation = json!({
        "run_id": input.run.run_id,
        "created_at": utc_now(),
        "engine_version": env!("CARGO_PKG_VERSION"),
        "run_pack_hash": stable_json_hash(&artifact_hashes),
        "artifact_hashes": artifact_hashes,
        "signature": null,
        "signature_algorithm": null,
    });
    if !input.verifier_models.is_empty() {
        attestation["verifier"] =
            json!({"method": input.verifier_method, "models": input.verifier_models});
    }
    if let Some(signer) = input.signer {
        let hash = attestation["run_pack_hash"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        attestation["signature"] = signer.sign(hash.as_bytes()).into();
        attestation["signature_algorithm"] = signer.algorithm().into();
        attestation["public_key"] = signer.public_key().into();
    }
    if let Some(replay) = input.replay {
        attestation["replay"] = Value::Object(write_replay_files(&output, replay)?);
    }
    write("attestation.json", &pretty(&attestation))?;
    Ok(output)
}

fn write_replay_files(output: &Path, replay: &ReplayFiles) -> io::Result<Map<String, Value>> {
    let mut hashes = Map::new();
    let inputs = pretty(&replay.inputs);
    fs::write(output.join(REPLAY_INPUTS), &inputs)?;
    hashes.insert(
        REPLAY_INPUTS.into(),
        sha256_prefixed(inputs.as_bytes()).into(),
    );
    if !replay.recordings.is_empty() {
        fs::create_dir_all(output.join(RECORDINGS_DIR))?;
    }
    for (name, text) in &replay.recordings {
        if !is_recording_name(name) {
            return Err(io::Error::other(format!("bad recording name: {name}")));
        }
        fs::write(output.join(RECORDINGS_DIR).join(name), text)?;
        hashes.insert(
            format!("{RECORDINGS_DIR}/{name}"),
            sha256_prefixed(text.as_bytes()).into(),
        );
    }
    Ok(hashes)
}

/// `<64 lowercase hex>.json`, the only names a recording may have.
fn is_recording_name(name: &str) -> bool {
    name.strip_suffix(".json").is_some_and(|stem| {
        stem.len() == 64
            && stem
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}

/// Where a `replay` entry may point: `replay.json` or `http/<key>.json`.
fn replay_path(dir: &Path, name: &str) -> Option<PathBuf> {
    if name == REPLAY_INPUTS {
        return Some(dir.join(REPLAY_INPUTS));
    }
    let file = name.strip_prefix(RECORDINGS_DIR)?.strip_prefix('/')?;
    is_recording_name(file).then(|| dir.join(RECORDINGS_DIR).join(file))
}

/// `run_pack_hash` from a pack's `attestation.json`.
///
/// # Errors
/// An unreadable or malformed attestation.
pub fn read_run_pack_hash(run_pack_dir: &Path) -> io::Result<String> {
    let text = fs::read_to_string(run_pack_dir.join("attestation.json"))?;
    let attestation: Value = serde_json::from_str(&text).map_err(io::Error::other)?;
    attestation
        .get("run_pack_hash")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| io::Error::other("attestation has no run_pack_hash"))
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
    if let Some(replay) = attestation.get("replay").and_then(Value::as_object) {
        for (name, expected) in replay {
            match replay_path(run_pack_dir, name) {
                None => mismatched.push(name.clone()),
                Some(path) if !path.exists() => missing.push(name.clone()),
                Some(path) if hash_file(&path).ok().as_deref() != expected.as_str() => {
                    mismatched.push(name.clone());
                }
                Some(_) => {}
            }
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
