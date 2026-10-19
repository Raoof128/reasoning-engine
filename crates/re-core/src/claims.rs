//! Claim typing and sentence splitting (port of verifiable/claims.py).
//!
//! Only the pure parts live here. Record types, evidence matching and
//! verification move to `re-verify`.

use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

static DIGIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d").expect("valid regex"));
static WHITESPACE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\s\x1c-\x1f]+").expect("valid regex"));

/// Domains whose claims carry medium risk.
pub const RISKY_DOMAINS: &[&str] = &["security", "medicine", "law"];

/// Rough epistemic category of a sentence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimType {
    /// Contains a decimal digit.
    Quantitative,
    /// States a cause.
    Causal,
    /// Says what should happen.
    Normative,
    /// Hedged.
    Hypothesis,
    /// Everything else.
    Empirical,
}

impl ClaimType {
    /// Whether a claim of this type needs a citation.
    #[must_use]
    pub fn requires_citation(self) -> bool {
        !matches!(self, Self::Normative | Self::Hypothesis)
    }
}

/// Classify a sentence. Matching is by substring, exactly as in Python, so
/// `mustard` counts as normative and `mayor` as a hypothesis.
#[must_use]
pub fn claim_type(text: &str) -> ClaimType {
    if DIGIT.is_match(text) {
        return ClaimType::Quantitative;
    }
    let lowered = text.to_lowercase();
    let has = |terms: &[&str]| terms.iter().any(|t| lowered.contains(t));
    if has(&["causes", "caused", "leads to", "because"]) {
        ClaimType::Causal
    } else if has(&["should", "must", "ought"]) {
        ClaimType::Normative
    } else if has(&["may", "might", "could", "likely"]) {
        ClaimType::Hypothesis
    } else {
        ClaimType::Empirical
    }
}

/// Split a draft into trimmed, non-empty sentences.
///
/// Python splits on `(?<=[.!?])\s+`. The `regex` crate has no lookbehind, so
/// this walks maximal whitespace runs and splits where the preceding
/// character is `.`, `!` or `?`. Both split only at the start of a maximal
/// run, so the results are identical.
#[must_use]
pub fn split_sentences(text: &str) -> Vec<&str> {
    let text = crate::pystr::py_strip(text);
    let mut parts = Vec::new();
    let mut start = 0;
    for run in WHITESPACE.find_iter(text) {
        let ends_sentence = text[..run.start()]
            .chars()
            .next_back()
            .is_some_and(|c| matches!(c, '.' | '!' | '?'));
        if ends_sentence {
            parts.push(&text[start..run.start()]);
            start = run.end();
        }
    }
    parts.push(&text[start..]);
    parts
        .into_iter()
        .map(crate::pystr::py_strip)
        .filter(|p| !p.is_empty())
        .collect()
}

/// A claim extracted from a draft, before any verification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DraftClaim {
    /// `claim_0001`, `claim_0002`, ...
    pub claim_id: String,
    /// The sentence without trailing `.`, `!` or `?`.
    pub text: String,
    /// Epistemic category.
    pub claim_type: ClaimType,
    /// `high` for quantitative and causal claims, else `medium`.
    pub importance: String,
    /// `medium` in risky domains, else `low`.
    pub risk_level: String,
    /// Whether verification must find a supporting source.
    pub requires_citation: bool,
}

/// Turn a draft into one claim per sentence.
#[must_use]
pub fn extract_draft_claims(text: &str, domain: &str) -> Vec<DraftClaim> {
    split_sentences(text)
        .into_iter()
        .enumerate()
        .map(|(i, sentence)| {
            let kind = claim_type(sentence);
            DraftClaim {
                claim_id: format!("claim_{:04}", i + 1),
                text: sentence.trim_end_matches(['.', '!', '?']).to_string(),
                claim_type: kind,
                importance: if matches!(kind, ClaimType::Quantitative | ClaimType::Causal) {
                    "high"
                } else {
                    "medium"
                }
                .to_string(),
                risk_level: if RISKY_DOMAINS.contains(&domain) {
                    "medium"
                } else {
                    "low"
                }
                .to_string(),
                requires_citation: kind.requires_citation(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_only_after_terminators() {
        assert_eq!(
            split_sentences("  A b. C!D?  E.\n\nF  "),
            ["A b.", "C!D?", "E.", "F"]
        );
        assert_eq!(split_sentences("   "), Vec::<&str>::new());
    }

    #[test]
    fn python_whitespace_separates_sentences() {
        // Python's `\s` and `strip()` include \x1c..\x1f; Rust's do not.
        assert_eq!(split_sentences("A b.\u{1c}C d!\u{1d}"), ["A b.", "C d!"]);
    }

    #[test]
    fn unicode_digits_are_quantitative_like_python() {
        // Python's \d matches any Unicode Nd digit, including Persian digits.
        assert_eq!(claim_type("نرخ ۳۰ درصد کاهش یافت"), ClaimType::Quantitative);
    }
}
