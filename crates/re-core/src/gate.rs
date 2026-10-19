//! Quality gate for verifiable research runs (port of verifiable/quality.py).
//!
//! `checked_at` is a timestamp, so it is added by `re-verify`, not here.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Verification statuses that block a final report.
pub const BLOCKING_STATUSES: &[&str] = &["unsupported", "needs_more_evidence", "contradicted"];

/// The parts of a claim the gate reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GateClaim {
    /// Claim identifier.
    pub claim_id: String,
    /// Whether the claim must be supported by evidence.
    #[serde(default)]
    pub requires_citation: bool,
}

/// The parts of a verification the gate reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GateVerification {
    /// Claim this verification belongs to.
    pub claim_id: String,
    /// Verifier verdict, e.g. `supported`.
    pub support_status: String,
}

/// Gate verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateResult {
    /// Nothing blocks and nothing to warn about.
    Pass,
    /// Nothing blocks, but evidence gaps remain.
    PassWithWarnings,
    /// At least one cited claim is unsupported or unverified.
    Blocked,
}

/// Gate outcome, without the timestamp.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GateOutcome {
    /// `gate_<run_id>`.
    pub gate_id: String,
    /// Run the gate checked.
    pub run_id: String,
    /// Verdict.
    pub result: GateResult,
    /// Reasons the run is blocked.
    pub blocking_failures: Vec<String>,
    /// Non-blocking concerns.
    pub warnings: Vec<String>,
}

/// Evaluate claims and verifications for a run.
///
/// When a claim has several verifications the last one wins, as with
/// Python's dict construction.
#[must_use]
pub fn run_quality_gate(
    run_id: &str,
    claims: &[GateClaim],
    verifications: &[GateVerification],
    gap_count: usize,
) -> GateOutcome {
    let by_claim: HashMap<&str, &GateVerification> = verifications
        .iter()
        .map(|v| (v.claim_id.as_str(), v))
        .collect();

    let blocked = claims.iter().filter(|c| c.requires_citation).any(|c| {
        by_claim
            .get(c.claim_id.as_str())
            .is_none_or(|v| BLOCKING_STATUSES.contains(&v.support_status.as_str()))
    });

    let mut blocking_failures = Vec::new();
    if blocked {
        blocking_failures.push("unsupported factual claims remain".to_string());
    }
    let mut warnings = Vec::new();
    if gap_count > 0 {
        warnings.push("unresolved evidence gaps are present".to_string());
    }
    let result = if !blocking_failures.is_empty() {
        GateResult::Blocked
    } else if !warnings.is_empty() {
        GateResult::PassWithWarnings
    } else {
        GateResult::Pass
    };

    GateOutcome {
        gate_id: format!("gate_{run_id}"),
        run_id: run_id.to_string(),
        result,
        blocking_failures,
        warnings,
    }
}
