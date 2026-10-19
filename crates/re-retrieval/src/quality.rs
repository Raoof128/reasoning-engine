//! Profile-aware source quality (plan Phase 9, section 6.12).
//!
//! [`classify`] maps a provider's publication data ([`QualityHints`]) to
//! the `source_type` names the profiles use: `systematic_review`,
//! `clinical_guideline`, `preprint`, `peer_reviewed_article`,
//! `technical_report`, `dataset`, `book`. [`apply`] then rewrites each
//! record's `source_type`, flags retractions (`risk_flags += ["retracted"]`),
//! adds 0.1 to the score of a profile's preferred types and takes 0.2 from
//! its weak ones, and re-ranks. Records a provider says nothing about keep
//! their adapter's `source_type`.
//!
//! This is opt-in: the default pipeline keeps Python's labels and scores.

use crate::base::py_round;
use re_model::records::{EvidenceRecord, QualityHints};

/// Score bonus for a profile's preferred source types.
pub const PREFERRED_BONUS: f64 = 0.1;
/// Score penalty for a profile's weak source types.
pub const WEAK_PENALTY: f64 = 0.2;
/// The flag a retracted record carries.
pub const RETRACTED: &str = "retracted";

/// The profile `source_type` for `hints`, if they say enough.
#[must_use]
pub fn classify(hints: &QualityHints) -> Option<&'static str> {
    let types: Vec<String> = hints
        .publication_types
        .iter()
        .map(|t| t.to_ascii_lowercase().replace(['_', '-'], " "))
        .collect();
    let has = |needle: &str| types.iter().any(|t| t.contains(needle));
    let venue = hints.venue_type.as_deref().unwrap_or_default();
    if has("systematic review") || has("meta analysis") {
        Some("systematic_review")
    } else if has("guideline") {
        Some("clinical_guideline")
    } else if has("preprint") || venue == "repository" {
        Some("preprint")
    } else if has("report") && !has("case report") {
        Some("technical_report")
    } else if has("dataset") {
        Some("dataset")
    } else if has("book") {
        Some("book")
    } else if has("article") || has("review") || has("conference") || has("letter") {
        Some("peer_reviewed_article")
    } else {
        None
    }
}

/// Classify, flag, rescore and re-rank `evidence` for a profile.
pub fn apply(evidence: &mut [EvidenceRecord], preferred: &[&str], weak: &[&str]) {
    for record in evidence.iter_mut() {
        if let Some(kind) = classify(&record.hints) {
            record.source_type = kind.to_string();
        }
        if record.hints.retracted == Some(true) && !record.risk_flags.iter().any(|f| f == RETRACTED)
        {
            record.risk_flags.push(RETRACTED.to_string());
        }
        let source_type = record.source_type.as_str();
        let mut score = record.score;
        if preferred.contains(&source_type) {
            score += PREFERRED_BONUS;
        }
        if weak.contains(&source_type) {
            score -= WEAK_PENALTY;
        }
        record.score = py_round(score, 4);
    }
    // Stable: equal scores keep the aggregator's order.
    evidence.sort_by(|a, b| b.score.total_cmp(&a.score));
    for (record, rank) in evidence.iter_mut().zip(1i64..) {
        record.evidence_id = format!("ev_{rank:04}");
        record.rank = rank;
    }
}

/// Whether a record may support a claim (retracted work may not).
#[must_use]
pub fn citable(record: &EvidenceRecord) -> bool {
    !record.risk_flags.iter().any(|f| f == RETRACTED)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::base::{RecordInput, build_record};

    fn hints(types: &[&str], venue: Option<&str>, retracted: Option<bool>) -> QualityHints {
        QualityHints {
            publication_types: types.iter().map(|t| (*t).to_string()).collect(),
            venue_type: venue.map(str::to_string),
            retracted,
            ..QualityHints::default()
        }
    }

    #[test]
    fn provider_types_map_to_profile_types() {
        let cases: [(&[&str], Option<&str>, Option<&str>); 14] = [
            (
                &["Systematic Review", "Journal Article"],
                None,
                Some("systematic_review"),
            ),
            (&["Meta-Analysis"], None, Some("systematic_review")),
            (&["Practice Guideline"], None, Some("clinical_guideline")),
            (&["Preprint"], None, Some("preprint")),
            (&["preprint"], Some("repository"), Some("preprint")),
            (&["article"], Some("repository"), Some("preprint")),
            (&["report"], None, Some("technical_report")),
            (
                &["Case Reports", "Journal Article"],
                None,
                Some("peer_reviewed_article"),
            ),
            (&["dataset"], Some("repository"), Some("preprint")),
            (&["dataset"], None, Some("dataset")),
            (&["book-chapter"], Some("ebook platform"), Some("book")),
            (&["research-article"], None, Some("peer_reviewed_article")),
            (
                &["conference-paper"],
                Some("conference"),
                Some("peer_reviewed_article"),
            ),
            (&[], None, None),
        ];
        for (types, venue, expected) in cases {
            assert_eq!(
                classify(&hints(types, venue, None)),
                expected,
                "{types:?} {venue:?}"
            );
        }
    }

    #[test]
    fn profiles_rescore_and_retractions_are_flagged() {
        let record = |title: &str, score: f64, h: QualityHints| {
            let mut r = build_record(RecordInput {
                title: title.into(),
                score,
                ..RecordInput::new("run", "openalex", "q", 1)
            });
            r.hints = h;
            r
        };
        let mut evidence = vec![
            record("preprint", 0.70, hints(&["preprint"], None, None)),
            record("review", 0.65, hints(&["Systematic Review"], None, None)),
            record(
                "retracted",
                0.60,
                hints(&["article"], Some("journal"), Some(true)),
            ),
            record("unknown", 0.50, QualityHints::default()),
        ];
        apply(&mut evidence, &["systematic_review"], &["preprint"]);
        let order: Vec<(&str, f64)> = evidence
            .iter()
            .map(|r| (r.title.as_str(), r.score))
            .collect();
        assert_eq!(
            order,
            [
                ("review", 0.75),
                ("retracted", 0.6),
                ("preprint", 0.5),
                ("unknown", 0.5)
            ]
        );
        assert_eq!(evidence[0].evidence_id, "ev_0001");
        assert_eq!(evidence[3].rank, 4);
        assert_eq!(evidence[1].risk_flags, [RETRACTED]);
        assert!(!citable(&evidence[1]));
        assert_eq!(
            evidence[3].source_type, "peer_reviewed_article",
            "kept from the adapter"
        );
        apply(&mut evidence, &[], &[]);
        assert_eq!(evidence[1].risk_flags, [RETRACTED], "flagged once");
    }
}
