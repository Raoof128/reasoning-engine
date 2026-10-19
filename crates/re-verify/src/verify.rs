//! Claim verification (port of `claims.verify_claims`).
//!
//! The [`Verifier`] trait is the seam for the Phase 5 entailment verifier;
//! [`LexicalOverlap`] is Python's term-overlap MVP, verbatim.

use std::collections::HashSet;
use std::sync::LazyLock;

use re_model::clock::utc_now;
use re_model::ids::short_hex;
use re_model::records::{ClaimRecord, EvidenceRecord, VerificationRecord};
use re_retrieval::base::py_round;
use regex::Regex;

use crate::error::VerifyError;

static WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new("[a-z0-9]+").expect("valid regex"));

/// Overlap at or above this is `supported`.
pub const SUPPORTED_THRESHOLD: f64 = 0.65;
/// Overlap at or above this is `partially_supported`.
pub const PARTIAL_THRESHOLD: f64 = 0.35;

/// Judges claims against evidence.
pub trait Verifier: Send + Sync {
    /// Stored in each record's `method`.
    fn method(&self) -> &'static str;

    /// Verify every claim, updating its `status`, `evidence_ids` and
    /// `confidence` in place like Python, and return one record per claim.
    fn verify(
        &self,
        claims: &mut [ClaimRecord],
        evidence: &[EvidenceRecord],
    ) -> Vec<VerificationRecord>;

    /// Pinned model identities, recorded in run packs so a run can be
    /// reproduced. Empty for the model-free lexical tier.
    fn models(&self) -> Vec<String> {
        Vec::new()
    }
}

/// Every tier name `REASONING_ENGINE_VERIFIER` accepts.
pub const TIERS: [&str; 3] = ["lexical", "hybrid", "nli"];

fn tier(name: Option<&str>) -> Result<&'static str, VerifyError> {
    let name = name
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .unwrap_or("lexical");
    TIERS
        .into_iter()
        .find(|t| *t == name)
        .ok_or_else(|| VerifyError::UnknownVerifier(name.to_string()))
}

/// Load the named tier: `lexical` (default), or `hybrid` / `nli`, which need
/// the `semantic` feature and may download their models first.
///
/// # Errors
/// An unknown name, a model tier in a build without `semantic`, or a model
/// that cannot be loaded (including offline with no model installed).
#[allow(
    clippy::unused_async,
    reason = "awaits model downloads when built with semantic"
)]
pub async fn load_verifier(
    name: Option<&str>,
    models_dir: &std::path::Path,
    offline: bool,
) -> Result<Box<dyn Verifier>, VerifyError> {
    let _ = (models_dir, offline);
    match tier(name)? {
        "lexical" => Ok(Box::new(LexicalOverlap)),
        #[cfg(feature = "semantic")]
        "hybrid" => {
            let embedder = crate::models::OnnxEmbedder::load(models_dir, offline).await?;
            Ok(Box::new(crate::hybrid::HybridVerifier::new(embedder)))
        }
        #[cfg(feature = "semantic")]
        "nli" => {
            let embedder = crate::models::OnnxEmbedder::load(models_dir, offline).await?;
            let nli = crate::models::OnnxNli::load(models_dir, offline).await?;
            Ok(Box::new(crate::nli::NliVerifier::new(embedder, nli)))
        }
        other => Err(VerifyError::NeedsSemantic(other.to_string())),
    }
}

/// The model-free verifier named by `REASONING_ENGINE_VERIFIER` (`lexical`
/// when unset or empty); see [`load_verifier`] for the model tiers.
///
/// # Errors
/// An unknown name, or a model tier (use [`load_verifier`]).
pub fn verifier_from_name(name: Option<&str>) -> Result<Box<dyn Verifier>, VerifyError> {
    match tier(name)? {
        "lexical" => Ok(Box::new(LexicalOverlap)),
        other => Err(VerifyError::NeedsSemantic(other.to_string())),
    }
}

fn terms(text: &str) -> HashSet<&str> {
    WORD.find_iter(text)
        .map(|m| m.as_str())
        .filter(|t| t.len() > 3)
        .collect()
}

/// Share of the claim's 4+ character ASCII terms that the snippet contains.
#[must_use]
pub fn overlap_score(claim_text: &str, snippet: &str) -> f64 {
    let (claim, snippet) = (claim_text.to_lowercase(), snippet.to_lowercase());
    let claim_terms = terms(&claim);
    if claim_terms.is_empty() {
        return 0.0;
    }
    let snippet_terms = terms(&snippet);
    claim_terms.intersection(&snippet_terms).count() as f64 / claim_terms.len() as f64
}

/// Deterministic term overlap: Python's `term_overlap_mvp`.
#[derive(Debug, Clone, Copy, Default)]
pub struct LexicalOverlap;

struct Verdict {
    status: &'static str,
    cite: bool,
    rationale: &'static str,
    missing: Option<&'static str>,
}

fn verdict(claim: &ClaimRecord, best: Option<f64>) -> Verdict {
    let v = |status, cite, rationale, missing| Verdict {
        status,
        cite,
        rationale,
        missing,
    };
    if !claim.requires_citation {
        let status = if claim.claim_type == "hypothesis" {
            "hypothesis"
        } else {
            "not_verifiable"
        };
        return v(
            status,
            false,
            "Claim does not require factual citation support.",
            None,
        );
    }
    match best {
        None => v(
            "needs_more_evidence",
            false,
            "No evidence was available for this claim.",
            Some("Retrieve source evidence that directly supports the claim."),
        ),
        Some(score) if score >= SUPPORTED_THRESHOLD => v(
            "supported",
            true,
            "Evidence snippet overlaps the claim strongly enough for MVP support.",
            None,
        ),
        Some(score) if score >= PARTIAL_THRESHOLD => v(
            "partially_supported",
            true,
            "Evidence is relevant but does not support the exact claim.",
            Some("Find direct evidence for the full claim."),
        ),
        Some(_) => v(
            "unsupported",
            false,
            "Available evidence does not support the claim.",
            Some("Find direct evidence or remove the claim from final mode."),
        ),
    }
}

impl Verifier for LexicalOverlap {
    fn method(&self) -> &'static str {
        "term_overlap_mvp"
    }

    fn verify(
        &self,
        claims: &mut [ClaimRecord],
        evidence: &[EvidenceRecord],
    ) -> Vec<VerificationRecord> {
        claims
            .iter_mut()
            .map(|claim| {
                // Python sorts by score, descending and stable: the first
                // evidence item with the highest score wins ties.
                let mut best: Option<(f64, &EvidenceRecord)> = None;
                for item in evidence {
                    let score = overlap_score(&claim.text, &item.snippet);
                    if best.is_none_or(|(top, _)| score > top) {
                        best = Some((score, item));
                    }
                }
                let best_score = best.map_or(0.0, |(score, _)| score);
                let decided = verdict(claim, best.map(|(score, _)| score));
                let evidence_ids: Vec<String> = match best {
                    Some((_, item)) if decided.cite => vec![item.evidence_id.clone()],
                    _ => Vec::new(),
                };
                let confidence = py_round(best_score, 2);
                claim.status = decided.status.into();
                claim.evidence_ids.clone_from(&evidence_ids);
                claim.confidence = Some(confidence);
                VerificationRecord {
                    verification_id: format!("ver_{}", short_hex(12)),
                    claim_id: claim.claim_id.clone(),
                    method: self.method().into(),
                    evidence_ids,
                    support_status: decided.status.into(),
                    support_rationale: decided.rationale.into(),
                    missing_evidence: decided.missing.map(Into::into),
                    contradictory_evidence_ids: Vec::new(),
                    confidence,
                    requires_human_review: matches!(
                        decided.status,
                        "partially_supported" | "unsupported" | "needs_more_evidence"
                    ),
                    verified_at: utc_now(),
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlap_counts_long_ascii_terms_once() {
        assert!(
            (overlap_score("The model model reward", "a reward MODEL") - 1.0).abs() < f64::EPSILON
        );
        assert!(overlap_score("tin a b", "tin").abs() < f64::EPSILON);
        assert!(
            (overlap_score("alpha beta1 gamma", "gamma only") - 1.0 / 3.0).abs() < f64::EPSILON
        );
    }

    #[test]
    fn unknown_verifier_names_are_rejected() {
        assert!(verifier_from_name(None).is_ok());
        assert!(verifier_from_name(Some(" lexical ")).is_ok());
        assert!(matches!(
            verifier_from_name(Some("nli")),
            Err(VerifyError::NeedsSemantic(_))
        ));
        assert!(matches!(
            verifier_from_name(Some("oracle")),
            Err(VerifyError::UnknownVerifier(_))
        ));
    }
}
