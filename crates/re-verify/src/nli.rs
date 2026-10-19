//! The `nli` verifier tier (plan section 6.1): the hybrid shortlist, then a
//! natural-language-inference cross-encoder judges each (snippet, claim)
//! pair as entailment, contradiction or neutral. This is the only tier that
//! can say `contradicted`.
//!
//! Mapping, unchanged from the plan so the gate and run-pack schema stay put:
//! - the strongest contradiction p >= 0.7, above every entailment -> `contradicted`
//! - entailment p >= 0.7 -> `supported`
//! - entailment in [0.4, 0.7) -> `partially_supported`
//! - otherwise `unsupported`; `needs_more_evidence` when nothing was shortlisted

use re_model::records::{ClaimRecord, EvidenceRecord, VerificationRecord};

use crate::hybrid::{Embedder, Outcome, finish, model_failure, no_evidence, shortlist, uncited};
use crate::verify::Verifier;

/// Entailment at or above this supports the claim.
pub const ENTAILED: f32 = 0.7;
/// Contradiction at or above this contradicts it.
pub const CONTRADICTED: f32 = 0.7;
/// Entailment at or above this partially supports it.
pub const PARTIAL: f32 = 0.4;

/// Probabilities for one (premise, hypothesis) pair.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NliScores {
    /// p(contradiction)
    pub contradiction: f32,
    /// p(entailment)
    pub entailment: f32,
    /// p(neutral)
    pub neutral: f32,
}

/// A cross-encoder that judges whether a premise entails a hypothesis.
pub trait NliModel: Send + Sync {
    /// Model identity for run-pack methodology.
    fn id(&self) -> String;
    /// Scores for each `(premise, hypothesis)` pair.
    ///
    /// # Errors
    /// Inference failures.
    fn classify(&self, pairs: &[(&str, &str)]) -> Result<Vec<NliScores>, String>;
}

/// The NLI tier: shortlist with `embedder`, judge with `nli`.
pub struct NliVerifier<E, N> {
    embedder: E,
    nli: N,
}

impl<E: Embedder, N: NliModel> NliVerifier<E, N> {
    /// A verifier using both models.
    pub fn new(embedder: E, nli: N) -> Self {
        Self { embedder, nli }
    }
}

/// Decide one claim from the scores of its shortlisted evidence.
fn decide(scored: &[(usize, NliScores)], evidence: &[EvidenceRecord]) -> Outcome {
    let id = |i: usize| evidence[i].evidence_id.clone();
    let best_entail = scored
        .iter()
        .max_by(|a, b| a.1.entailment.total_cmp(&b.1.entailment));
    let best_contra = scored
        .iter()
        .max_by(|a, b| a.1.contradiction.total_cmp(&b.1.contradiction));
    let (Some(&(ei, es)), Some(&(_, cs))) = (best_entail, best_contra) else {
        return no_evidence();
    };
    let contradictory: Vec<String> = scored
        .iter()
        .filter(|(_, s)| s.contradiction >= CONTRADICTED)
        .map(|&(i, _)| id(i))
        .collect();
    let outcome = |status, evidence_ids, rationale: String, missing, confidence: f32| Outcome {
        status,
        evidence_ids,
        contradictory: contradictory.clone(),
        rationale,
        missing,
        confidence: f64::from(confidence),
    };
    if cs.contradiction >= CONTRADICTED && cs.contradiction > es.entailment {
        return outcome(
            "contradicted",
            Vec::new(),
            format!(
                "Evidence contradicts the claim (NLI p={:.2}).",
                cs.contradiction
            ),
            Some("Resolve the contradiction or revise the claim."),
            cs.contradiction,
        );
    }
    if es.entailment >= ENTAILED {
        return outcome(
            "supported",
            vec![id(ei)],
            format!("Evidence entails the claim (NLI p={:.2}).", es.entailment),
            None,
            es.entailment,
        );
    }
    if es.entailment >= PARTIAL {
        return outcome(
            "partially_supported",
            vec![id(ei)],
            format!(
                "Evidence partly entails the claim (NLI p={:.2}).",
                es.entailment
            ),
            Some("Find direct evidence for the full claim."),
            es.entailment,
        );
    }
    outcome(
        "unsupported",
        Vec::new(),
        format!(
            "No shortlisted evidence entails the claim (best NLI p={:.2}).",
            es.entailment
        ),
        Some("Find direct evidence or remove the claim from final mode."),
        es.entailment,
    )
}

impl<E: Embedder, N: NliModel> Verifier for NliVerifier<E, N> {
    fn method(&self) -> &'static str {
        "nli_cross_encoder"
    }

    fn models(&self) -> Vec<String> {
        vec![self.embedder.id(), self.nli.id()]
    }

    fn can_contradict(&self) -> bool {
        true
    }

    fn verify(
        &self,
        claims: &mut [ClaimRecord],
        evidence: &[EvidenceRecord],
    ) -> Vec<VerificationRecord> {
        let texts: Vec<&str> = claims.iter().map(|c| c.text.as_str()).collect();
        let shortlists = shortlist(&self.embedder, &texts, evidence);
        claims
            .iter_mut()
            .enumerate()
            .map(|(i, claim)| {
                if !claim.requires_citation {
                    return uncited(claim, self.method());
                }
                let outcome = match &shortlists {
                    Err(error) => model_failure(error),
                    Ok(lists) if lists[i].is_empty() => no_evidence(),
                    Ok(lists) => {
                        let pairs: Vec<(&str, &str)> = lists[i]
                            .iter()
                            .map(|c| (evidence[c.index].snippet.as_str(), claim.text.as_str()))
                            .collect();
                        match self.nli.classify(&pairs) {
                            Ok(probabilities) => {
                                let judged: Vec<(usize, NliScores)> = lists[i]
                                    .iter()
                                    .map(|c| c.index)
                                    .zip(probabilities)
                                    .collect();
                                decide(&judged, evidence)
                            }
                            Err(error) => model_failure(&error),
                        }
                    }
                };
                finish(claim, self.method(), outcome)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hybrid::tests::{FakeEmbedder, claim, evidence};

    /// Contradiction when the premise says "not"/"fails", entailment when
    /// they share most words, neutral otherwise. Enough to exercise the
    /// mapping deterministically.
    struct FakeNli;

    impl NliModel for FakeNli {
        fn id(&self) -> String {
            "fake-nli".into()
        }

        fn classify(&self, pairs: &[(&str, &str)]) -> Result<Vec<NliScores>, String> {
            Ok(pairs
                .iter()
                .map(|(premise, hypothesis)| {
                    let p = premise.to_lowercase();
                    let shared = hypothesis
                        .split_whitespace()
                        .filter(|w| p.contains(&w.to_lowercase()))
                        .count();
                    let overlap =
                        shared as f32 / hypothesis.split_whitespace().count().max(1) as f32;
                    if (p.contains(" not ") || p.contains("fails")) && overlap > 0.5 {
                        NliScores {
                            contradiction: 0.9,
                            entailment: 0.05,
                            neutral: 0.05,
                        }
                    } else if overlap > 0.8 {
                        NliScores {
                            contradiction: 0.02,
                            entailment: 0.9,
                            neutral: 0.08,
                        }
                    } else if overlap > 0.5 {
                        NliScores {
                            contradiction: 0.1,
                            entailment: 0.5,
                            neutral: 0.4,
                        }
                    } else {
                        NliScores {
                            contradiction: 0.1,
                            entailment: 0.1,
                            neutral: 0.8,
                        }
                    }
                })
                .collect())
        }
    }

    #[test]
    fn maps_nli_scores_to_statuses() {
        let ev = evidence(&[
            "process reward model scores each step",
            "the outcome reward model does not label each step",
            "garden tomato tips",
        ]);
        let verifier = NliVerifier::new(FakeEmbedder, FakeNli);
        let mut claims = vec![
            claim("process reward model scores each step", "empirical", true),
            claim("outcome reward model labels each step", "empirical", true),
            claim("reward model scores tomato safety", "empirical", true),
        ];
        let records = verifier.verify(&mut claims, &ev);
        let statuses: Vec<&str> = records.iter().map(|r| r.support_status.as_str()).collect();
        assert_eq!(statuses[0], "supported");
        assert_eq!(statuses[1], "contradicted");
        assert_eq!(
            records[1].contradictory_evidence_ids,
            [ev[1].evidence_id.clone()]
        );
        assert!(records[1].requires_human_review);
        assert_eq!(claims[1].contradiction_ids, [ev[1].evidence_id.clone()]);
        assert!(matches!(statuses[2], "unsupported" | "partially_supported"));
        assert_eq!(records[0].method, "nli_cross_encoder");
    }

    #[test]
    fn empty_evidence_needs_more() {
        let mut claims = vec![claim("anything at all", "empirical", true)];
        let records = NliVerifier::new(FakeEmbedder, FakeNli).verify(&mut claims, &[]);
        assert_eq!(records[0].support_status, "needs_more_evidence");
    }
}
