//! Verifier benchmark over a labelled claim/evidence set
//! (`tests/fixtures/verify/labelled_pairs.json`).
//!
//! Each pair is verified on its own, with its one evidence sentence as the
//! only evidence. Statuses map to labels strictly: `supported` is
//! supported, `contradicted` is contradicted, and everything else
//! (including `partially_supported`) is neutral.

use std::fmt::Write as _;

use re_model::records::{ClaimRecord, EvidenceRecord};
use re_retrieval::base::{RecordInput, build_record};
use serde::Deserialize;

use crate::verify::Verifier;

/// The three labels, in report order.
pub const LABELS: [&str; 3] = ["supported", "contradicted", "neutral"];

/// One labelled pair.
#[derive(Debug, Clone, Deserialize)]
pub struct Pair {
    /// Stable identifier.
    pub id: String,
    /// The claim to verify.
    pub claim: String,
    /// The only evidence sentence.
    pub evidence: String,
    /// Gold label: one of [`LABELS`].
    pub label: String,
}

/// The labelled set file.
#[derive(Debug, Clone, Deserialize)]
pub struct LabelledSet {
    /// The pairs.
    pub pairs: Vec<Pair>,
}

/// Map a verification status to a label.
#[must_use]
pub fn label_of(status: &str) -> &'static str {
    match status {
        "supported" => "supported",
        "contradicted" => "contradicted",
        _ => "neutral",
    }
}

fn index(label: &str) -> usize {
    LABELS.iter().position(|l| *l == label).unwrap_or(2)
}

/// Confusion matrix `[gold][predicted]` and the per-pair predictions.
#[derive(Debug, Clone)]
pub struct Report {
    /// Verifier method.
    pub method: String,
    /// Counts by gold row and predicted column, in [`LABELS`] order.
    pub confusion: [[usize; 3]; 3],
    /// `(pair id, gold, predicted, confidence)`.
    pub predictions: Vec<(String, String, String, f64)>,
}

impl Report {
    /// Precision for `label` (0 when nothing was predicted as it).
    #[must_use]
    pub fn precision(&self, label: &str) -> f64 {
        let j = index(label);
        let predicted: usize = (0..3).map(|i| self.confusion[i][j]).sum();
        ratio(self.confusion[j][j], predicted)
    }

    /// Recall for `label`.
    #[must_use]
    pub fn recall(&self, label: &str) -> f64 {
        let i = index(label);
        ratio(self.confusion[i][i], self.confusion[i].iter().sum())
    }

    /// F1 for `label`.
    #[must_use]
    pub fn f1(&self, label: &str) -> f64 {
        let (p, r) = (self.precision(label), self.recall(label));
        if p + r == 0.0 {
            0.0
        } else {
            2.0 * p * r / (p + r)
        }
    }

    /// Share of pairs labelled correctly.
    #[must_use]
    pub fn accuracy(&self) -> f64 {
        let correct: usize = (0..3).map(|i| self.confusion[i][i]).sum();
        ratio(correct, self.confusion.iter().flatten().sum())
    }

    /// A Markdown table: one row per label, then accuracy.
    #[must_use]
    pub fn table(&self) -> String {
        let mut out = format!(
            "### {}\n\n| Label | Precision | Recall | F1 |\n|---|---:|---:|---:|\n",
            self.method
        );
        for label in LABELS {
            let _ = writeln!(
                out,
                "| {label} | {:.3} | {:.3} | {:.3} |",
                self.precision(label),
                self.recall(label),
                self.f1(label)
            );
        }
        let _ = writeln!(
            out,
            "\nAccuracy {:.3} over {} pairs.",
            self.accuracy(),
            self.predictions.len()
        );
        out
    }
}

fn ratio(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

/// Verify every pair with `verifier` and tally the results.
///
/// # Panics
/// Never for valid pairs: claims are built with a fixed, valid type.
#[must_use]
pub fn evaluate(verifier: &dyn Verifier, pairs: &[Pair]) -> Report {
    let mut confusion = [[0usize; 3]; 3];
    let mut predictions = Vec::with_capacity(pairs.len());
    for pair in pairs {
        let evidence: Vec<EvidenceRecord> = vec![build_record(RecordInput {
            title: pair.id.clone(),
            snippet: pair.evidence.clone(),
            ..RecordInput::new("bench", "bench", "bench", 1)
        })];
        let mut claims = vec![
            ClaimRecord::new(
                "claim_0001",
                "bench",
                &pair.claim,
                "empirical",
                "general",
                "medium",
                "low",
                true,
                "bench",
            )
            .expect("empirical is a valid claim type"),
        ];
        let record = verifier.verify(&mut claims, &evidence).remove(0);
        let predicted = label_of(&record.support_status);
        confusion[index(&pair.label)][index(predicted)] += 1;
        predictions.push((
            pair.id.clone(),
            pair.label.clone(),
            predicted.to_string(),
            record.confidence,
        ));
    }
    Report {
        method: verifier.method().to_string(),
        confusion,
        predictions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verify::LexicalOverlap;

    fn set() -> LabelledSet {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/verify/labelled_pairs.json");
        serde_json::from_str(&std::fs::read_to_string(path).expect("labelled set"))
            .expect("valid set")
    }

    #[test]
    fn the_set_is_balanced_enough_and_well_formed() {
        let set = set();
        assert!(set.pairs.len() >= 100);
        for label in LABELS {
            assert!(
                set.pairs.iter().filter(|p| p.label == label).count() >= 25,
                "{label}"
            );
        }
        assert!(set.pairs.iter().all(|p| LABELS.contains(&p.label.as_str())));
    }

    #[test]
    fn lexical_never_finds_contradictions() {
        let report = evaluate(&LexicalOverlap, &set().pairs);
        assert!(
            report.recall("contradicted").abs() < f64::EPSILON,
            "lexical cannot say contradicted"
        );
        assert_eq!(report.predictions.len(), set().pairs.len());
        assert!(
            report
                .table()
                .contains("| contradicted | 0.000 | 0.000 | 0.000 |")
        );
    }
}
