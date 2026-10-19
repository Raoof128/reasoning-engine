//! The `hybrid` verifier tier (plan section 6.1): BM25 and embedding cosine
//! each rank the evidence for a claim, reciprocal-rank fusion merges them,
//! and the best cosine in the fused shortlist decides the status.
//!
//! Embedding similarity cannot see negation ("X reduces Y" and "X does not
//! reduce Y" embed almost identically), so this tier never returns
//! `contradicted`; that is the `nli` tier's job. The shortlist it builds is
//! also what the `nli` tier reads.

use re_core::rank::{RRF_K, bm25_scores, ranking, reciprocal_rank_fusion};
use re_model::clock::utc_now;
use re_model::ids::short_hex;
use re_model::records::{ClaimRecord, EvidenceRecord, VerificationRecord};
use re_retrieval::base::py_round;

use crate::verify::Verifier;

/// Snippets kept per claim after fusion.
pub const SHORTLIST: usize = 5;
/// Best shortlisted cosine at or above this is `supported`.
pub const HYBRID_SUPPORTED: f32 = 0.82;
/// Best shortlisted cosine at or above this is `partially_supported`.
pub const HYBRID_PARTIAL: f32 = 0.72;

/// Turns texts into unit-length vectors.
pub trait Embedder: Send + Sync {
    /// Model identity for run-pack methodology, e.g. `bge-small-en-v1.5-q@<sha>`.
    fn id(&self) -> String;
    /// One L2-normalised vector per text.
    ///
    /// # Errors
    /// Inference failures.
    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, String>;
}

/// Dot product, which is cosine for unit vectors.
#[must_use]
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// One shortlisted evidence item for a claim.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Candidate {
    /// Index into the evidence slice.
    pub index: usize,
    /// Fused RRF score.
    pub fused: f64,
    /// Claim-snippet cosine.
    pub cosine: f32,
}

/// Rank `evidence` for each claim and keep the top [`SHORTLIST`].
///
/// # Errors
/// Embedding failures.
pub fn shortlist(
    embedder: &dyn Embedder,
    claims: &[&str],
    evidence: &[EvidenceRecord],
) -> Result<Vec<Vec<Candidate>>, String> {
    if evidence.is_empty() {
        return Ok(vec![Vec::new(); claims.len()]);
    }
    let snippets: Vec<&str> = evidence.iter().map(|e| e.snippet.as_str()).collect();
    let snippet_vectors = embedder.embed(&snippets)?;
    let claim_vectors = embedder.embed(claims)?;
    Ok(claims
        .iter()
        .zip(&claim_vectors)
        .map(|(claim, claim_vector)| {
            let cosines: Vec<f32> = snippet_vectors
                .iter()
                .map(|v| cosine(claim_vector, v))
                .collect();
            let lexical = ranking(&bm25_scores(claim, &snippets));
            let semantic = ranking(&cosines.iter().map(|&c| f64::from(c)).collect::<Vec<_>>());
            reciprocal_rank_fusion(&[lexical, semantic], RRF_K)
                .into_iter()
                .take(SHORTLIST)
                .map(|(index, fused)| Candidate {
                    index,
                    fused,
                    cosine: cosines[index],
                })
                .collect()
        })
        .collect())
}

/// The hybrid tier over any [`Embedder`].
pub struct HybridVerifier<E> {
    embedder: E,
}

impl<E: Embedder> HybridVerifier<E> {
    /// A verifier using `embedder`.
    pub fn new(embedder: E) -> Self {
        Self { embedder }
    }
}

/// The record shared by every tier for claims that need no citation.
pub(crate) fn uncited(claim: &mut ClaimRecord, method: &str) -> VerificationRecord {
    let status = if claim.claim_type == "hypothesis" {
        "hypothesis"
    } else {
        "not_verifiable"
    };
    finish(
        claim,
        method,
        Outcome {
            status,
            evidence_ids: Vec::new(),
            contradictory: Vec::new(),
            rationale: "Claim does not require factual citation support.".into(),
            missing: None,
            confidence: 0.0,
        },
    )
}

/// A tier's decision for one claim.
pub(crate) struct Outcome {
    pub status: &'static str,
    pub evidence_ids: Vec<String>,
    pub contradictory: Vec<String>,
    pub rationale: String,
    pub missing: Option<&'static str>,
    pub confidence: f64,
}

/// Apply `outcome` to the claim (as Python's verifier mutates it) and build
/// the verification record.
pub(crate) fn finish(
    claim: &mut ClaimRecord,
    method: &str,
    outcome: Outcome,
) -> VerificationRecord {
    let confidence = py_round(outcome.confidence, 2);
    claim.status = outcome.status.into();
    claim.evidence_ids.clone_from(&outcome.evidence_ids);
    claim.contradiction_ids.clone_from(&outcome.contradictory);
    claim.confidence = Some(confidence);
    VerificationRecord {
        verification_id: format!("ver_{}", short_hex(12)),
        claim_id: claim.claim_id.clone(),
        method: method.into(),
        evidence_ids: outcome.evidence_ids,
        support_status: outcome.status.into(),
        support_rationale: outcome.rationale,
        missing_evidence: outcome.missing.map(Into::into),
        contradictory_evidence_ids: outcome.contradictory,
        confidence,
        requires_human_review: matches!(
            outcome.status,
            "partially_supported" | "unsupported" | "needs_more_evidence" | "contradicted"
        ),
        verified_at: utc_now(),
    }
}

/// The outcome for a cited claim with no evidence at all.
pub(crate) fn no_evidence() -> Outcome {
    Outcome {
        status: "needs_more_evidence",
        evidence_ids: Vec::new(),
        contradictory: Vec::new(),
        rationale: "No evidence was available for this claim.".into(),
        missing: Some("Retrieve source evidence that directly supports the claim."),
        confidence: 0.0,
    }
}

/// A failed model run degrades to `needs_more_evidence` with the reason,
/// never to a silent `supported`.
pub(crate) fn model_failure(error: &str) -> Outcome {
    Outcome {
        status: "needs_more_evidence",
        evidence_ids: Vec::new(),
        contradictory: Vec::new(),
        rationale: format!("Verifier model failed: {error}"),
        missing: Some("Re-run verification once the model loads."),
        confidence: 0.0,
    }
}

impl<E: Embedder> Verifier for HybridVerifier<E> {
    fn method(&self) -> &'static str {
        "hybrid_bm25_embedding_rrf"
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
                    Ok(lists) => match lists[i].iter().max_by(|a, b| a.cosine.total_cmp(&b.cosine))
                    {
                        None => no_evidence(),
                        Some(best) => decide(*best, evidence),
                    },
                };
                finish(claim, self.method(), outcome)
            })
            .collect()
    }
}

fn decide(best: Candidate, evidence: &[EvidenceRecord]) -> Outcome {
    let id = vec![evidence[best.index].evidence_id.clone()];
    let confidence = f64::from(best.cosine).clamp(0.0, 1.0);
    let (status, evidence_ids, rationale, missing) = if best.cosine >= HYBRID_SUPPORTED {
        (
            "supported",
            id,
            "Evidence is semantically close to the claim (hybrid retrieval).",
            None,
        )
    } else if best.cosine >= HYBRID_PARTIAL {
        (
            "partially_supported",
            id,
            "Evidence is related to the claim but not close enough to support it fully.",
            Some("Find direct evidence for the full claim."),
        )
    } else {
        (
            "unsupported",
            Vec::new(),
            "No shortlisted evidence is close to the claim.",
            Some("Find direct evidence or remove the claim from final mode."),
        )
    };
    Outcome {
        status,
        evidence_ids,
        contradictory: Vec::new(),
        rationale: format!("{rationale} Best cosine {:.3}.", best.cosine),
        missing,
        confidence,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use re_retrieval::base::{RecordInput, build_record};

    /// Bag-of-words vectors over a fixed vocabulary: deterministic,
    /// model-free, and similar texts really are close.
    pub struct FakeEmbedder;

    const VOCAB: [&str; 12] = [
        "reward", "process", "outcome", "model", "step", "answer", "label", "cost", "garden",
        "tomato", "safety", "audit",
    ];

    impl Embedder for FakeEmbedder {
        fn id(&self) -> String {
            "fake-bow".into()
        }

        fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, String> {
            Ok(texts
                .iter()
                .map(|t| {
                    let lower = t.to_lowercase();
                    let mut v: Vec<f32> = VOCAB
                        .iter()
                        .map(|w| lower.matches(w).count() as f32)
                        .collect();
                    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
                    if norm > 0.0 {
                        for x in &mut v {
                            *x /= norm;
                        }
                    }
                    v
                })
                .collect())
        }
    }

    pub fn evidence(snippets: &[&str]) -> Vec<EvidenceRecord> {
        snippets
            .iter()
            .enumerate()
            .map(|(i, s)| {
                build_record(RecordInput {
                    title: format!("E{i}"),
                    snippet: (*s).into(),
                    ..RecordInput::new("run", "openalex", "q", i + 1)
                })
            })
            .collect()
    }

    pub fn claim(text: &str, kind: &str, cited: bool) -> ClaimRecord {
        ClaimRecord::new(
            "claim_0001",
            "run",
            text,
            kind,
            "general",
            "medium",
            "low",
            cited,
            "draft",
        )
        .expect("claim")
    }

    #[test]
    fn shortlist_fuses_lexical_and_semantic_ranks() {
        let ev = evidence(&[
            "gardening tomato tips",
            "process reward model scores each step",
            "outcome reward answer",
        ]);
        let lists =
            shortlist(&FakeEmbedder, &["process reward model step"], &ev).expect("shortlist");
        assert_eq!(lists[0][0].index, 1);
        assert!(lists[0].len() <= SHORTLIST);
        assert!(lists[0][0].cosine > 0.9);
    }

    #[test]
    fn statuses_follow_the_best_cosine() {
        let ev = evidence(&[
            "process reward model scores each step",
            "gardening tomato tips",
        ]);
        let verifier = HybridVerifier::new(FakeEmbedder);
        let mut claims = vec![
            claim("A process reward model scores each step", "empirical", true),
            claim("Tomato gardening needs safety audits", "empirical", true),
            claim("Models may improve", "hypothesis", false),
        ];
        let records = verifier.verify(&mut claims, &ev);
        assert_eq!(records[0].support_status, "supported");
        assert_eq!(records[0].evidence_ids, [ev[0].evidence_id.clone()]);
        assert_eq!(records[1].support_status, "unsupported");
        assert_eq!(records[2].support_status, "hypothesis");
        assert_eq!(claims[0].status, "supported");
        assert_eq!(records[0].method, "hybrid_bm25_embedding_rrf");
    }

    #[test]
    fn no_evidence_and_model_failures_never_support() {
        struct Broken;
        impl Embedder for Broken {
            fn id(&self) -> String {
                "broken".into()
            }
            fn embed(&self, _: &[&str]) -> Result<Vec<Vec<f32>>, String> {
                Err("onnx exploded".into())
            }
        }
        let mut claims = vec![claim("Rewards help", "empirical", true)];
        assert_eq!(
            HybridVerifier::new(FakeEmbedder).verify(&mut claims, &[])[0].support_status,
            "needs_more_evidence"
        );
        let failed = HybridVerifier::new(Broken).verify(&mut claims, &evidence(&["x"]));
        assert_eq!(failed[0].support_status, "needs_more_evidence");
        assert!(failed[0].support_rationale.contains("onnx exploded"));
    }
}
