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

/// Confusion matrix `[gold][predicted]` and the per-pair predictions.
#[derive(Debug, Clone)]
pub struct Report {
    /// Verifier method.
    pub method: String,
    /// The labels, in matrix order; the last also catches anything else.
    pub labels: [&'static str; 3],
    /// Counts by gold row and predicted column, in `labels` order.
    pub confusion: [[usize; 3]; 3],
    /// `(pair id, gold, predicted, confidence)`.
    pub predictions: Vec<(String, String, String, f64)>,
}

impl Report {
    fn index(&self, label: &str) -> usize {
        self.labels.iter().position(|l| *l == label).unwrap_or(2)
    }

    /// Precision for `label` (0 when nothing was predicted as it).
    #[must_use]
    pub fn precision(&self, label: &str) -> f64 {
        let j = self.index(label);
        let predicted: usize = (0..3).map(|i| self.confusion[i][j]).sum();
        ratio(self.confusion[j][j], predicted)
    }

    /// Recall for `label`.
    #[must_use]
    pub fn recall(&self, label: &str) -> f64 {
        let i = self.index(label);
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
        for label in self.labels {
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
        let index = |label: &str| LABELS.iter().position(|l| *l == label).unwrap_or(2);
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
        labels: LABELS,
        confusion,
        predictions,
    }
}

/// Labels of the full-text set, where `partially_supported` is its own
/// class.
#[cfg(feature = "fulltext")]
pub const FULL_TEXT_LABELS: [&str; 3] = ["supported", "partially_supported", "unsupported"];

/// One claim about one paper of a [`FullTextSet`].
#[cfg(feature = "fulltext")]
#[derive(Debug, Clone, Deserialize)]
pub struct FullTextPair {
    /// Stable identifier.
    pub id: String,
    /// The claim to verify.
    pub claim: String,
    /// Key into [`FullTextSet::documents`].
    pub document: String,
    /// Gold label: one of [`FULL_TEXT_LABELS`].
    pub label: String,
    /// Where the support is, for reviewers.
    #[serde(default)]
    pub note: String,
}

/// Claims about open-access papers
/// (`tests/fixtures/verify/fulltext_pairs.json`).
#[cfg(feature = "fulltext")]
#[derive(Debug, Clone, Deserialize)]
pub struct FullTextSet {
    /// Document key to its file, relative to the set's directory.
    pub documents: std::collections::BTreeMap<String, String>,
    /// The pairs.
    pub pairs: Vec<FullTextPair>,
}

/// Three reports over `set`, whose documents are read from `dir`:
///
/// 1. abstract only: the paper's abstract (and front matter, for PDFs) as
///    the record's snippet, as the pipeline verified before full text;
/// 2. abstract in chunks: the same text split like full text, without the
///    background cap, to separate shorter premises from more text;
/// 3. full text, through [`crate::chunks::verify`].
///
/// # Errors
/// A document that cannot be read or parsed.
///
/// # Panics
/// Never: claims are built with a fixed, valid type.
#[cfg(feature = "fulltext")]
pub fn evaluate_full_text(
    verifier: &dyn Verifier,
    set: &FullTextSet,
    dir: &std::path::Path,
) -> Result<[Report; 3], String> {
    use re_retrieval::fulltext::{europepmc_xml, pdf};
    let mut papers = std::collections::BTreeMap::new();
    for (key, file) in &set.documents {
        let path = dir.join(file);
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let is_pdf = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("pdf"));
        let sections = if is_pdf {
            pdf::parse(&bytes)
        } else {
            europepmc_xml::parse(&String::from_utf8_lossy(&bytes))
        }
        .map_err(|e| format!("{}: {e}", path.display()))?;
        // Relabelled so the chunked-abstract run is not capped.
        let summary: Vec<re_retrieval::fulltext::Section> = sections
            .iter()
            .filter(|s| matches!(s.label.as_str(), "front" | "abstract"))
            .map(|s| re_retrieval::fulltext::Section {
                label: "other".into(),
                ..s.clone()
            })
            .collect();
        let text: Vec<&str> = summary.iter().map(|s| s.text.as_str()).collect();
        papers.insert(
            key.clone(),
            (
                text.join("\n"),
                crate::chunks::chunk(&summary),
                crate::chunks::chunk(&sections),
            ),
        );
    }
    let mut reports = Vec::with_capacity(3);
    for (run, name) in ["abstract only", "abstract in chunks", "full text"]
        .into_iter()
        .enumerate()
    {
        let mut confusion = [[0usize; 3]; 3];
        let mut predictions = Vec::with_capacity(set.pairs.len());
        for pair in &set.pairs {
            let (summary, summary_chunks, chunks) = papers
                .get(&pair.document)
                .ok_or_else(|| format!("{}: unknown document {}", pair.id, pair.document))?;
            let evidence = vec![build_record(RecordInput {
                title: pair.document.clone(),
                snippet: summary.clone(),
                ..RecordInput::new("bench", "bench", "bench", 1)
            })];
            let id = evidence[0].evidence_id.clone();
            let texts = match run {
                0 => std::collections::BTreeMap::new(),
                1 => std::collections::BTreeMap::from([(id, summary_chunks.clone())]),
                _ => std::collections::BTreeMap::from([(id, chunks.clone())]),
            };
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
            let record = crate::chunks::verify(verifier, &mut claims, &evidence, &texts).remove(0);
            let predicted = match record.support_status.as_str() {
                "supported" => "supported",
                "partially_supported" => "partially_supported",
                _ => "unsupported",
            };
            let index = |label: &str| {
                FULL_TEXT_LABELS
                    .iter()
                    .position(|l| *l == label)
                    .unwrap_or(2)
            };
            confusion[index(&pair.label)][index(predicted)] += 1;
            predictions.push((
                pair.id.clone(),
                pair.label.clone(),
                predicted.to_string(),
                record.confidence,
            ));
        }
        reports.push(Report {
            method: format!("{} ({name})", verifier.method()),
            labels: FULL_TEXT_LABELS,
            confusion,
            predictions,
        });
    }
    reports.try_into().map_err(|_| "three reports".to_string())
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

#[cfg(all(test, feature = "fulltext"))]
mod full_text_tests {
    use super::*;
    use crate::verify::LexicalOverlap;

    #[test]
    fn the_full_text_set_is_well_formed_and_scores_three_ways() {
        let dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/verify");
        let set: FullTextSet = serde_json::from_str(
            &std::fs::read_to_string(dir.join("fulltext_pairs.json")).expect("set"),
        )
        .expect("valid set");
        assert!(set.pairs.len() >= 40);
        for document in set.documents.keys() {
            for label in FULL_TEXT_LABELS {
                assert!(
                    set.pairs
                        .iter()
                        .any(|p| &p.document == document && p.label == label),
                    "{document} has a {label} claim"
                );
            }
        }
        let reports = evaluate_full_text(&LexicalOverlap, &set, &dir).expect("documents parse");
        assert_eq!(
            reports.each_ref().map(|r| r.method.as_str()),
            [
                "term_overlap_mvp (abstract only)",
                "term_overlap_mvp (abstract in chunks)",
                "term_overlap_mvp (full text)"
            ]
        );
        assert!(
            reports
                .iter()
                .all(|r| r.predictions.len() == set.pairs.len())
        );
        assert!(
            reports[2].accuracy() > reports[0].accuracy(),
            "full text helps even the lexical tier"
        );
    }
}
