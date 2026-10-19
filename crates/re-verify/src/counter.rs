//! Contradiction search (`graph` feature, plan Phase 10).
//!
//! After verification, each supported claim (up to [`COUNTER_CLAIMS`]) is
//! searched again with its first contradiction query
//! (`re_core::planning::negate`), and the verifier judges the claim
//! against what comes back. Only a tier that can say `contradicted` (NLI)
//! takes part; for the others a search could only ever "survive", which
//! would be noise.
//!
//! - Nothing contradicts it: confidence `+0.05` (at most 1.0) and the
//!   rationale gains `Survived contradiction search.`
//! - Something does: the claim keeps its status but is flagged for human
//!   review, and the contrary work is named in the rationale and in the
//!   report's `## Counter-view`.
//!
//! Counter-search results are not added to the run's evidence (their
//! identifiers would collide with the run's own); what was found is in the
//! `counter_search` provenance event.

use std::fmt::Write as _;

use re_core::planning::negate::negated_queries;
use re_model::records::{ClaimRecord, EvidenceRecord, VerificationRecord};
use re_retrieval::AcademicSearchAggregator;
use re_retrieval::base::py_round;
use serde::Serialize;

use crate::verify::Verifier;

/// Supported claims searched for contradiction per run.
pub const COUNTER_CLAIMS: usize = 5;
/// Results per contradiction search.
pub const COUNTER_LIMIT: usize = 5;
/// Confidence a claim gains by surviving.
pub const SURVIVAL_BONUS: f64 = 0.05;

/// Work found to contradict a claim.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Contrary {
    /// Title.
    pub title: String,
    /// Source adapter.
    pub source: String,
    /// DOI or URL, when known.
    pub link: Option<String>,
}

/// One claim's contradiction search.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Finding {
    /// The claim searched.
    pub claim_id: String,
    /// The query used.
    pub query: String,
    /// Results the search returned.
    pub results: usize,
    /// What contradicted the claim, strongest first.
    pub contrary: Vec<Contrary>,
}

fn contrary(record: &EvidenceRecord) -> Contrary {
    Contrary {
        title: record.title.clone(),
        source: record.source_adapter.clone(),
        link: record
            .doi
            .as_ref()
            .map(|doi| format!("https://doi.org/{doi}"))
            .or_else(|| record.url.clone()),
    }
}

/// Search for contradictions of the supported claims and update their
/// verifications in place.
pub async fn search(
    aggregator: &AcademicSearchAggregator,
    run_id: &str,
    verifier: &dyn Verifier,
    claims: &mut [ClaimRecord],
    verifications: &mut [VerificationRecord],
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for verification in verifications
        .iter_mut()
        .filter(|v| v.support_status == "supported")
        .take(COUNTER_CLAIMS)
    {
        let Some(claim) = claims
            .iter_mut()
            .find(|c| c.claim_id == verification.claim_id)
        else {
            continue;
        };
        let Some(query) = negated_queries(&claim.text).into_iter().next() else {
            continue;
        };
        let found = aggregator.search(run_id, &query, COUNTER_LIMIT).await;
        let mut probe = claim.clone();
        let judged = if found.evidence.is_empty() {
            None
        } else {
            verifier
                .verify(std::slice::from_mut(&mut probe), &found.evidence)
                .pop()
        };
        let against: Vec<Contrary> = judged
            .filter(|j| j.support_status == "contradicted")
            .map(|j| {
                j.contradictory_evidence_ids
                    .iter()
                    .filter_map(|id| found.evidence.iter().find(|e| &e.evidence_id == id))
                    .map(contrary)
                    .collect()
            })
            .unwrap_or_default();
        if against.is_empty() {
            let confidence = py_round((verification.confidence + SURVIVAL_BONUS).min(1.0), 2);
            verification.confidence = confidence;
            claim.confidence = Some(confidence);
            verification
                .support_rationale
                .push_str(" Survived contradiction search.");
        } else {
            verification.requires_human_review = true;
            let _ = write!(
                verification.support_rationale,
                " Contradiction search found contrary work: {}.",
                against[0].title
            );
        }
        findings.push(Finding {
            claim_id: claim.claim_id.clone(),
            query,
            results: found.evidence.len(),
            contrary: against,
        });
    }
    findings
}

/// The report's `## Counter-view`: contradicted and partially supported
/// claims, and supported claims a contradiction search contested, each
/// with its strongest contrary evidence. Empty when there is none.
#[must_use]
pub fn counter_view(
    claims: &[ClaimRecord],
    verifications: &[VerificationRecord],
    evidence: &[EvidenceRecord],
    findings: &[Finding],
) -> String {
    let mut lines = Vec::new();
    for claim in claims {
        let Some(verification) = verifications
            .iter()
            .rev()
            .find(|v| v.claim_id == claim.claim_id)
        else {
            continue;
        };
        let status = verification.support_status.as_str();
        let finding = findings.iter().find(|f| f.claim_id == claim.claim_id);
        let ledger_contrary = verification
            .contradictory_evidence_ids
            .iter()
            .find_map(|id| evidence.iter().find(|e| &e.evidence_id == id))
            .map(contrary);
        let strongest =
            ledger_contrary.or_else(|| finding.and_then(|f| f.contrary.first().cloned()));
        let contested = finding.is_some_and(|f| !f.contrary.is_empty());
        if !(matches!(status, "contradicted" | "partially_supported") || contested) {
            continue;
        }
        let label = if contested && status == "supported" {
            "supported, contested"
        } else {
            status
        };
        let against = strongest.map_or_else(
            || {
                verification
                    .missing_evidence
                    .clone()
                    .unwrap_or_else(|| "no contrary evidence found".into())
            },
            |c| match c.link {
                Some(link) => format!("contrary: {} ({}, <{link}>)", c.title, c.source),
                None => format!("contrary: {} ({})", c.title, c.source),
            },
        );
        lines.push(format!("- [{label}] {}: {against}", claim.text));
    }
    if lines.is_empty() {
        return String::new();
    }
    format!("\n## Counter-view\n\n{}\n", lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use re_retrieval::base::{RecordInput, build_record};

    fn claim(id: &str, text: &str) -> ClaimRecord {
        ClaimRecord::new(
            id,
            "run",
            text,
            "empirical",
            "general",
            "medium",
            "low",
            true,
            "draft",
        )
        .unwrap()
    }

    fn verification(claim_id: &str, status: &str, contrary: &[&str]) -> VerificationRecord {
        VerificationRecord {
            verification_id: format!("ver_{claim_id}"),
            claim_id: claim_id.into(),
            method: "nli_cross_encoder".into(),
            evidence_ids: Vec::new(),
            support_status: status.into(),
            support_rationale: "r".into(),
            missing_evidence: (status == "partially_supported")
                .then(|| "Find direct evidence for the full claim.".into()),
            contradictory_evidence_ids: contrary.iter().map(|s| (*s).to_string()).collect(),
            confidence: 0.8,
            requires_human_review: status != "supported",
            verified_at: "2026-10-19T00:00:00Z".into(),
        }
    }

    #[test]
    fn the_counter_view_lists_contested_claims_with_contrary_work() {
        let mut against = build_record(RecordInput {
            title: "Naps do nothing".into(),
            ..RecordInput::new("run", "crossref", "q", 1)
        });
        against.evidence_id = "ev_0002".into();
        against.doi = Some("10.1/x".into());
        let claims = [
            claim("c1", "Naps improve power."),
            claim("c2", "Naps reduce soreness."),
            claim("c3", "Naps lower fatigue."),
            claim("c4", "Naps aid recovery."),
        ];
        let verifications = [
            verification("c1", "contradicted", &["ev_0002"]),
            verification("c2", "partially_supported", &[]),
            verification("c3", "supported", &[]),
            verification("c4", "supported", &[]),
        ];
        let findings = [
            Finding {
                claim_id: "c3".into(),
                query: "Naps do not lower fatigue".into(),
                results: 3,
                contrary: vec![Contrary {
                    title: "No fatigue benefit".into(),
                    source: "openalex".into(),
                    link: None,
                }],
            },
            Finding {
                claim_id: "c4".into(),
                query: "Naps do not aid recovery".into(),
                results: 2,
                contrary: Vec::new(),
            },
        ];
        assert_eq!(
            counter_view(&claims, &verifications, &[against], &findings),
            "\n## Counter-view\n\n\
             - [contradicted] Naps improve power.: contrary: Naps do nothing (crossref, <https://doi.org/10.1/x>)\n\
             - [partially_supported] Naps reduce soreness.: Find direct evidence for the full claim.\n\
             - [supported, contested] Naps lower fatigue.: contrary: No fatigue benefit (openalex)\n"
        );
        assert_eq!(
            counter_view(&claims[3..], &verifications[3..], &[], &findings),
            ""
        );
    }
}
