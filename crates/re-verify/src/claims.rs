//! Draft-to-claim extraction (port of `claims.extract_claims`).

use re_core::claims::{ClaimType, extract_draft_claims};
use re_model::records::{ClaimRecord, ModelError};

fn type_name(kind: ClaimType) -> &'static str {
    match kind {
        ClaimType::Quantitative => "quantitative",
        ClaimType::Causal => "causal",
        ClaimType::Normative => "normative",
        ClaimType::Hypothesis => "hypothesis",
        ClaimType::Empirical => "empirical",
    }
}

/// One `needs_more_evidence` claim per sentence of `text`.
///
/// # Errors
/// Never in practice: every extracted type is a valid claim type.
pub fn extract_claims(
    run_id: &str,
    text: &str,
    domain: &str,
) -> Result<Vec<ClaimRecord>, ModelError> {
    extract_draft_claims(text, domain)
        .into_iter()
        .map(|draft| {
            ClaimRecord::new(
                &draft.claim_id,
                run_id,
                &draft.text,
                type_name(draft.claim_type),
                domain,
                &draft.importance,
                &draft.risk_level,
                draft.requires_citation,
                "draft",
            )
        })
        .collect()
}
