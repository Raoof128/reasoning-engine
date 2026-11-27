//! The `nli` verifier tier (plan section 6.1): the hybrid shortlist, then a
//! natural-language-inference cross-encoder judges each (snippet, claim)
//! pair as entailment, contradiction or neutral. This is the only tier that
//! can say `contradicted`.
//!
//! A snippet is usually a whole abstract, and a sentence-pair NLI model
//! rarely calls a claim entailed by a ten-sentence premise even when one of
//! those sentences states it. So each shortlisted snippet is judged as a
//! whole and also in windows of one and two sentences (the ones sharing the
//! most terms with the claim), and the snippet's entailment is the highest
//! of them. This is the granularity fix of `SummaC` (Laban et al., TACL 2022).
//! Contradiction is still read from the whole snippet only, and is capped
//! at one minus the snippet's entailment. A single sentence out of context
//! ("the treatment failed in mice") contradicts too readily: on `SciFact`,
//! taking contradiction from sentences as well cost more precision than it
//! gained recall (see `CHANGELOG.md`, 2026-11-27).
//!
//! Mapping, unchanged from the plan so the gate and run-pack schema stay put:
//! - the strongest contradiction p >= 0.7, above every entailment -> `contradicted`
//! - entailment p >= 0.7 -> `supported`
//! - entailment in [0.4, 0.7) -> `partially_supported`
//! - otherwise `unsupported`; `needs_more_evidence` when nothing was shortlisted

use re_model::records::{ClaimRecord, EvidenceRecord, VerificationRecord};

#[cfg(feature = "fa")]
use crate::hybrid::Candidate;
use crate::hybrid::{Embedder, Outcome, finish, model_failure, no_evidence, shortlist, uncited};
use crate::verify::Verifier;

/// Entailment at or above this supports the claim.
pub const ENTAILED: f32 = 0.7;
/// Contradiction at or above this contradicts it.
pub const CONTRADICTED: f32 = 0.7;
/// Entailment at or above this partially supports it.
pub const PARTIAL: f32 = 0.4;

/// Sentence windows judged per snippet, besides the snippet as a whole.
/// Keeps the cost of one claim bounded whatever the abstract's length.
pub const WINDOWS_PER_SNIPPET: usize = 6;

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

/// Lowercased letter-and-digit runs of three characters or more.
fn terms(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.chars().count() >= 3)
        .map(str::to_lowercase)
        .collect()
}

/// The premises to judge `claim` against for one snippet: the snippet
/// itself first, then up to [`WINDOWS_PER_SNIPPET`] one- and two-sentence
/// windows, the ones sharing the most distinct terms with the claim, in
/// document order. A snippet of one sentence yields only itself.
fn windows(snippet: &str, claim: &str) -> Vec<String> {
    let mut out = vec![snippet.to_string()];
    let sentences = re_core::claims::split_sentences(snippet);
    if sentences.len() < 2 {
        return out;
    }
    let wanted: std::collections::HashSet<String> = terms(claim).into_iter().collect();
    let overlap = |text: &str| {
        let seen: std::collections::HashSet<String> = terms(text).into_iter().collect();
        wanted.intersection(&seen).count()
    };
    // (shared terms, is a single sentence, position, text)
    let mut candidates: Vec<(usize, bool, usize, String)> = Vec::new();
    for (i, sentence) in sentences.iter().enumerate() {
        let position = candidates.len();
        candidates.push((overlap(sentence), true, position, (*sentence).to_string()));
        if let Some(next) = sentences.get(i + 1) {
            let pair = format!("{sentence} {next}");
            let position = candidates.len();
            candidates.push((overlap(&pair), false, position, pair));
        }
    }
    // Most shared terms first; a single sentence beats the pair containing
    // it at equal overlap, because the shorter premise is the sharper one.
    candidates.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| b.1.cmp(&a.1))
            .then_with(|| a.2.cmp(&b.2))
    });
    candidates.truncate(WINDOWS_PER_SNIPPET);
    candidates.sort_by_key(|c| c.2);
    out.extend(candidates.into_iter().filter(|c| c.0 > 0).map(|c| c.3));
    out
}

/// One snippet's scores from the scores of its premises, the whole snippet
/// first: the highest entailment of any of them, and the whole snippet's
/// contradiction, capped so the two cannot both be near one.
fn pooled(judged: &[NliScores]) -> NliScores {
    let entailment = judged.iter().map(|s| s.entailment).fold(0.0_f32, f32::max);
    let contradiction = judged
        .first()
        .map_or(0.0, |whole| whole.contradiction)
        .min(1.0 - entailment);
    NliScores {
        contradiction,
        entailment,
        neutral: (1.0 - entailment - contradiction).max(0.0),
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

/// Whether the claim or any shortlisted snippet is Persian, which the
/// English NLI model cannot read.
#[cfg(feature = "fa")]
fn non_english(claim: &str, shortlist: &[Candidate], evidence: &[EvidenceRecord]) -> bool {
    re_core::fa::is_persian(claim)
        || shortlist
            .iter()
            .any(|c| re_core::fa::is_persian(&evidence[c.index].snippet))
}

/// The hybrid tier's decision, saying why NLI was not used.
#[cfg(feature = "fa")]
fn english_only(
    shortlist: &[Candidate],
    evidence: &[EvidenceRecord],
    cutoffs: (f32, f32),
) -> Outcome {
    let best = shortlist
        .iter()
        .max_by(|a, b| a.cosine.total_cmp(&b.cosine))
        .copied();
    let Some(best) = best else {
        return no_evidence();
    };
    let mut outcome = crate::hybrid::decide(best, evidence, cutoffs);
    outcome.rationale.push_str(
        " The NLI model reads English only, so this non-English pair was judged by the \
         hybrid tier.",
    );
    outcome
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

    fn probabilities(&self, premise: &str, hypothesis: &str) -> Option<[f64; 3]> {
        let scores = self.nli.classify(&[(premise, hypothesis)]).ok()?.pop()?;
        Some([
            f64::from(scores.entailment),
            f64::from(scores.contradiction),
            f64::from(scores.neutral),
        ])
    }

    fn verify(
        &self,
        claims: &mut [ClaimRecord],
        evidence: &[EvidenceRecord],
    ) -> Vec<VerificationRecord> {
        let texts: Vec<&str> = claims.iter().map(|c| c.text.as_str()).collect();
        let shortlists = shortlist(&self.embedder, &texts, evidence);
        #[cfg(feature = "fa")]
        let cutoffs = crate::hybrid::routed(&self.embedder, &texts, evidence).cutoffs();
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
                    #[cfg(feature = "fa")]
                    Ok(lists) if non_english(&claim.text, &lists[i], evidence) => {
                        english_only(&lists[i], evidence, cutoffs)
                    }
                    Ok(lists) => {
                        // Every window of every shortlisted snippet, judged
                        // in one batch per claim.
                        let cut: Vec<Vec<String>> = lists[i]
                            .iter()
                            .map(|c| windows(&evidence[c.index].snippet, &claim.text))
                            .collect();
                        let pairs: Vec<(&str, &str)> = cut
                            .iter()
                            .flatten()
                            .map(|w| (w.as_str(), claim.text.as_str()))
                            .collect();
                        match self.nli.classify(&pairs) {
                            Ok(probabilities) => {
                                let mut scores = probabilities.into_iter();
                                let judged: Vec<(usize, NliScores)> = lists[i]
                                    .iter()
                                    .zip(&cut)
                                    .map(|(c, ws)| {
                                        let per_window: Vec<NliScores> =
                                            scores.by_ref().take(ws.len()).collect();
                                        (c.index, pooled(&per_window))
                                    })
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
    fn one_sentence_of_a_long_snippet_can_entail() {
        // The claim is one sentence of five. Judged whole, the fake model
        // sees too little overlap; judged by sentence, it entails.
        let abstract_ = "We study verifiers for model reasoning. Prior work labels only \
                         final answers. Process reward model scores each step. We report \
                         results on three suites. Limitations are discussed at length.";
        let ev = evidence(&[abstract_]);
        let verifier = NliVerifier::new(FakeEmbedder, FakeNli);
        let mut claims = vec![claim(
            "process reward model scores each step",
            "empirical",
            true,
        )];
        let records = verifier.verify(&mut claims, &ev);
        assert_eq!(records[0].support_status, "supported");
        assert_eq!(records[0].evidence_ids, [ev[0].evidence_id.clone()]);
    }

    #[test]
    fn a_contradicting_sentence_does_not_outvote_an_entailing_one() {
        let abstract_ = "The tool fails on legacy inputs and does not handle them. The \
                         process reward model scores each step. Further work is planned.";
        let ev = evidence(&[abstract_]);
        let verifier = NliVerifier::new(FakeEmbedder, FakeNli);
        let mut claims = vec![claim(
            "process reward model scores each step",
            "empirical",
            true,
        )];
        let records = verifier.verify(&mut claims, &ev);
        assert_eq!(records[0].support_status, "supported");
        assert_eq!(records[0].contradictory_evidence_ids, Vec::<String>::new());
    }

    #[test]
    fn windows_keep_the_whole_snippet_first_and_stay_bounded() {
        let snippet = (1..=20)
            .map(|i| format!("Sentence {i} mentions reward models."))
            .collect::<Vec<_>>()
            .join(" ");
        let cut = windows(&snippet, "reward models");
        assert_eq!(cut[0], snippet);
        assert!(cut.len() <= 1 + WINDOWS_PER_SNIPPET);
        assert_eq!(windows("Only one sentence here.", "one sentence").len(), 1);
        // No shared term, no window: nothing to gain from judging it.
        assert_eq!(
            windows("Alpha beta. Gamma delta.", "unrelated words").len(),
            1
        );
    }

    #[test]
    fn empty_evidence_needs_more() {
        let mut claims = vec![claim("anything at all", "empirical", true)];
        let records = NliVerifier::new(FakeEmbedder, FakeNli).verify(&mut claims, &[]);
        assert_eq!(records[0].support_status, "needs_more_evidence");
    }
}
