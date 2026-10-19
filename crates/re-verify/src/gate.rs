//! The quality gate with its timestamp (port of `quality.run_quality_gate`).
//! The pure decision lives in `re_core::gate`.

use re_core::gate::{GateClaim, GateVerification, run_quality_gate};
use re_model::clock::utc_now;
use re_model::records::{ClaimRecord, VerificationRecord};
use serde_json::{Value, json};

fn finish(
    run_id: &str,
    claims: &[GateClaim],
    verifications: &[GateVerification],
    gap_count: usize,
) -> Value {
    let outcome = run_quality_gate(run_id, claims, verifications, gap_count);
    // Python's dict literal order, then `checked_at`.
    json!({
        "gate_id": outcome.gate_id,
        "run_id": outcome.run_id,
        "result": outcome.result,
        "blocking_failures": outcome.blocking_failures,
        "warnings": outcome.warnings,
        "checked_at": utc_now(),
    })
}

/// Gate over in-memory records.
#[must_use]
pub fn gate_records(
    run_id: &str,
    claims: &[ClaimRecord],
    verifications: &[VerificationRecord],
    gap_count: usize,
) -> Value {
    let claims: Vec<GateClaim> = claims
        .iter()
        .map(|c| GateClaim {
            claim_id: c.claim_id.clone(),
            requires_citation: c.requires_citation,
        })
        .collect();
    let verifications: Vec<GateVerification> = verifications
        .iter()
        .map(|v| GateVerification {
            claim_id: v.claim_id.clone(),
            support_status: v.support_status.clone(),
        })
        .collect();
    finish(run_id, &claims, &verifications, gap_count)
}

fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// Gate over stored payloads, as `run_quality_gate_tool` does. Missing
/// fields read as Python's `dict.get` would: empty IDs and falsy flags.
#[must_use]
pub fn gate_payloads(
    run_id: &str,
    claims: &[Value],
    verifications: &[Value],
    gap_count: usize,
) -> Value {
    let claims: Vec<GateClaim> = claims
        .iter()
        .map(|c| GateClaim {
            claim_id: text(c, "claim_id"),
            requires_citation: c
                .get("requires_citation")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        })
        .collect();
    let verifications: Vec<GateVerification> = verifications
        .iter()
        .map(|v| GateVerification {
            claim_id: text(v, "claim_id"),
            support_status: text(v, "support_status"),
        })
        .collect();
    finish(run_id, &claims, &verifications, gap_count)
}
