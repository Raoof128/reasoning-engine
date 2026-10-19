//! Verify claims against open-access full text (`fulltext` feature, plan
//! Phase 9).
//!
//! [`chunk`] splits a paper's sections into sentence-aligned chunks of at
//! most [`CHUNK_WORDS`] words, each keeping its section label. [`verify`]
//! then judges each claim with any [`Verifier`] tier: a record with full
//! text is replaced by its [`TOP_CHUNKS`] chunks that share the most terms
//! with the claim (the abstract included), and a record without keeps its
//! snippet as before.
//!
//! Support found only in a paper's front matter, abstract or introduction
//! is background, not a finding: the claim is checked again against the
//! rest of the paper, and if nothing there supports it the verdict is
//! capped at `partially_supported`, naming the section. Chunk identifiers
//! (`ev_0001#4`) never leave this module: verdicts cite the record.

use std::collections::BTreeMap;

use re_model::records::{ClaimRecord, EvidenceRecord, VerificationRecord};
use re_retrieval::fulltext::Section;

use crate::verify::{Verifier, overlap_score};

/// Words per chunk: two or three sentences. NLI cross-encoders are trained
/// on short premises; on the full-text bench, 40 and 60 words score alike
/// and 100 or 200 words lose a fifth of the accuracy.
pub const CHUNK_WORDS: usize = 60;
/// Chunks per paper given to the verifier for each claim.
pub const TOP_CHUNKS: usize = 3;
/// Sections whose support alone is capped.
pub const BACKGROUND: [&str; 3] = ["front", "abstract", "introduction"];

/// One chunk of a paper.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    /// The section label (see `re_retrieval::fulltext::LABELS`).
    pub section: String,
    /// The text.
    pub text: String,
}

/// Sentences of `text`: split after `.`, `!` or `?` followed by
/// whitespace, and at paragraph breaks.
fn sentences(text: &str) -> Vec<&str> {
    let mut found = Vec::new();
    for paragraph in text.split('\n') {
        let mut start = 0;
        let bytes = paragraph.as_bytes();
        for (i, b) in bytes.iter().enumerate() {
            if matches!(b, b'.' | b'!' | b'?')
                && bytes.get(i + 1).is_some_and(u8::is_ascii_whitespace)
            {
                found.push(paragraph[start..=i].trim());
                start = i + 1;
            }
        }
        found.push(paragraph[start..].trim());
    }
    found.retain(|s| !s.is_empty());
    found
}

/// Split sections into sentence-aligned chunks of at most [`CHUNK_WORDS`]
/// words (a longer sentence is a chunk of its own).
#[must_use]
pub fn chunk(sections: &[Section]) -> Vec<Chunk> {
    let mut chunks = Vec::new();
    for section in sections {
        let mut text = String::new();
        let mut words = 0;
        for sentence in sentences(&section.text) {
            let count = sentence.split_whitespace().count();
            if words > 0 && words + count > CHUNK_WORDS {
                chunks.push(Chunk {
                    section: section.label.clone(),
                    text: std::mem::take(&mut text),
                });
                words = 0;
            }
            if !text.is_empty() {
                text.push(' ');
            }
            text.push_str(sentence);
            words += count;
        }
        if !text.is_empty() {
            chunks.push(Chunk {
                section: section.label.clone(),
                text,
            });
        }
    }
    chunks
}

/// The evidence the verifier sees for one claim, and where each chunk came
/// from.
struct View {
    records: Vec<EvidenceRecord>,
    /// Chunk identifier to (record identifier, section).
    origin: BTreeMap<String, (String, String)>,
}

impl View {
    fn build(
        claim: &str,
        evidence: &[EvidenceRecord],
        texts: &BTreeMap<String, Vec<Chunk>>,
        background: bool,
    ) -> Self {
        let mut records = Vec::new();
        let mut origin = BTreeMap::new();
        for record in evidence {
            let Some(chunks) = texts.get(&record.evidence_id) else {
                records.push(record.clone());
                continue;
            };
            let mut ranked: Vec<(usize, f64)> = chunks
                .iter()
                .enumerate()
                .filter(|(_, c)| background || !BACKGROUND.contains(&c.section.as_str()))
                .map(|(i, c)| (i, overlap_score(claim, &c.text)))
                .collect();
            // Stable: equal scores keep reading order.
            ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
            for (index, _) in ranked.into_iter().take(TOP_CHUNKS) {
                let id = format!("{}#{index}", record.evidence_id);
                origin.insert(
                    id.clone(),
                    (record.evidence_id.clone(), chunks[index].section.clone()),
                );
                let mut view = record.clone();
                view.evidence_id = id;
                view.snippet.clone_from(&chunks[index].text);
                records.push(view);
            }
        }
        Self { records, origin }
    }

    /// The sections behind `ids` that are chunks, in order, deduplicated.
    fn sections(&self, ids: &[String]) -> Vec<&str> {
        let mut found: Vec<&str> = Vec::new();
        for id in ids {
            if let Some((_, section)) = self.origin.get(id)
                && !found.contains(&section.as_str())
            {
                found.push(section);
            }
        }
        found
    }

    /// Whether every cited identifier is a background chunk.
    fn background_only(&self, ids: &[String]) -> bool {
        !ids.is_empty()
            && ids.iter().all(|id| {
                self.origin
                    .get(id)
                    .is_some_and(|(_, s)| BACKGROUND.contains(&s.as_str()))
            })
    }

    /// Rewrite chunk identifiers to their records, deduplicated.
    fn cite(&self, ids: &mut Vec<String>) {
        let mut cited: Vec<String> = Vec::with_capacity(ids.len());
        for id in ids.drain(..) {
            let record = self
                .origin
                .get(&id)
                .map_or(id, |(record, _)| record.clone());
            if !cited.contains(&record) {
                cited.push(record);
            }
        }
        *ids = cited;
    }
}

fn verify_one(verifier: &dyn Verifier, claim: &mut ClaimRecord, view: &View) -> VerificationRecord {
    verifier
        .verify(std::slice::from_mut(claim), &view.records)
        .remove(0)
}

/// Verify `claims` against `evidence`, using the chunks in `texts` (keyed
/// by evidence identifier) for records with full text. Without any full
/// text this is exactly `verifier.verify(claims, evidence)`.
#[must_use]
pub fn verify(
    verifier: &dyn Verifier,
    claims: &mut [ClaimRecord],
    evidence: &[EvidenceRecord],
    texts: &BTreeMap<String, Vec<Chunk>>,
) -> Vec<VerificationRecord> {
    let used: BTreeMap<String, Vec<Chunk>> = texts
        .iter()
        .filter(|(id, chunks)| !chunks.is_empty() && evidence.iter().any(|e| &e.evidence_id == *id))
        .map(|(id, chunks)| (id.clone(), chunks.clone()))
        .collect();
    if used.is_empty() {
        return verifier.verify(claims, evidence);
    }
    let mut records = Vec::with_capacity(claims.len());
    for claim in claims.iter_mut() {
        let mut view = View::build(&claim.text, evidence, &used, true);
        let mut record = verify_one(verifier, claim, &view);
        let mut capped = None;
        if record.support_status == "supported" && view.background_only(&record.evidence_ids) {
            let first = record.clone();
            let saved = claim.clone();
            let body = View::build(&claim.text, evidence, &used, false);
            let retried = verify_one(verifier, claim, &body);
            if retried.support_status == "supported" {
                record = retried;
                view = body;
            } else {
                *claim = saved;
                capped = view
                    .sections(&first.evidence_ids)
                    .first()
                    .map(|s| (*s).to_string());
                record = first;
            }
        }
        let sections: Vec<String> = view
            .sections(&record.evidence_ids)
            .into_iter()
            .map(str::to_string)
            .collect();
        view.cite(&mut record.evidence_ids);
        view.cite(&mut record.contradictory_evidence_ids);
        view.cite(&mut claim.evidence_ids);
        view.cite(&mut claim.contradiction_ids);
        if let Some(section) = capped {
            record.support_status = "partially_supported".into();
            record.support_rationale = format!(
                "{} Support comes only from the {section} of the full text.",
                record.support_rationale
            );
            record.missing_evidence = Some(format!(
                "Find support in the paper's methods or results, not its {section}."
            ));
            record.requires_human_review = true;
            claim.status = "partially_supported".into();
        } else if !sections.is_empty()
            && matches!(
                record.support_status.as_str(),
                "supported" | "partially_supported"
            )
        {
            record.support_rationale = format!(
                "{} Full text: {}.",
                record.support_rationale,
                sections.join(", ")
            );
        }
        records.push(record);
    }
    records
}

/// `evidence` with each full-text record's snippet followed by its whole
/// text, for checks that look for exact values (the numeric check).
#[must_use]
pub fn with_full_text(
    evidence: &[EvidenceRecord],
    texts: &BTreeMap<String, Vec<Chunk>>,
) -> Vec<EvidenceRecord> {
    evidence
        .iter()
        .map(|record| {
            let mut record = record.clone();
            if let Some(chunks) = texts.get(&record.evidence_id) {
                for chunk in chunks {
                    record.snippet.push('\n');
                    record.snippet.push_str(&chunk.text);
                }
            }
            record
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verify::LexicalOverlap;
    use re_retrieval::base::{RecordInput, build_record};

    fn section(label: &str, text: &str) -> Section {
        Section {
            label: label.into(),
            heading: label.into(),
            text: text.into(),
        }
    }

    fn claim(text: &str) -> ClaimRecord {
        ClaimRecord::new(
            "claim_0001",
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

    fn record(id: &str, snippet: &str) -> EvidenceRecord {
        let mut record = build_record(RecordInput {
            title: "Paper".into(),
            snippet: snippet.into(),
            ..RecordInput::new("run", "openalex", "q", 1)
        });
        record.evidence_id = id.into();
        record
    }

    #[test]
    fn chunks_follow_sentences_and_keep_their_section() {
        let long = "Word filler text. ".repeat(150);
        let chunks = chunk(&[
            section("abstract", "Short abstract. Second sentence?"),
            section("results", &long),
        ]);
        assert_eq!(chunks[0].text, "Short abstract. Second sentence?");
        assert_eq!(chunks[0].section, "abstract");
        assert!(chunks.len() >= 3, "450 words split into 200-word chunks");
        assert!(chunks[1..].iter().all(|c| c.section == "results"
            && c.text.split_whitespace().count() <= CHUNK_WORDS
            && c.text.ends_with('.')));
    }

    fn texts(id: &str, sections: &[Section]) -> BTreeMap<String, Vec<Chunk>> {
        BTreeMap::from([(id.to_string(), chunk(sections))])
    }

    #[test]
    fn support_from_the_results_is_cited_to_the_record() {
        let evidence = [record("ev_0001", "An unrelated abstract about sleep.")];
        let texts = texts(
            "ev_0001",
            &[
                section("abstract", "An unrelated abstract about sleep."),
                section(
                    "results",
                    "Daytime naps improved peak power output in athletes.",
                ),
            ],
        );
        let mut claims = [claim(
            "Daytime naps improved peak power output in athletes.",
        )];
        let records = verify(&LexicalOverlap, &mut claims, &evidence, &texts);
        assert_eq!(records[0].support_status, "supported");
        assert_eq!(records[0].evidence_ids, ["ev_0001"]);
        assert_eq!(claims[0].evidence_ids, ["ev_0001"]);
        assert!(
            records[0]
                .support_rationale
                .ends_with("Full text: results.")
        );

        let plain = verify(&LexicalOverlap, &mut claims, &evidence, &BTreeMap::new());
        assert_ne!(plain[0].support_status, "supported", "the abstract alone");
    }

    #[test]
    fn support_only_from_the_introduction_is_capped() {
        let evidence = [record("ev_0001", "Abstract text.")];
        let texts = texts(
            "ev_0001",
            &[
                section(
                    "introduction",
                    "Insufficient sleep impairs physical performance in athletes.",
                ),
                section("results", "Peak power rose after the nap."),
            ],
        );
        let mut claims = [claim(
            "Insufficient sleep impairs physical performance in athletes.",
        )];
        let records = verify(&LexicalOverlap, &mut claims, &evidence, &texts);
        assert_eq!(records[0].support_status, "partially_supported");
        assert_eq!(claims[0].status, "partially_supported");
        assert_eq!(records[0].evidence_ids, ["ev_0001"]);
        assert!(records[0].requires_human_review);
        assert!(
            records[0]
                .support_rationale
                .ends_with("Support comes only from the introduction of the full text."),
            "{}",
            records[0].support_rationale
        );
    }

    #[test]
    fn background_support_stands_when_the_body_agrees() {
        let evidence = [record("ev_0001", "Abstract text.")];
        let finding = "Daytime naps improved peak power output in athletes.";
        let texts = texts(
            "ev_0001",
            &[section("abstract", finding), section("results", finding)],
        );
        let mut claims = [claim(finding)];
        let records = verify(&LexicalOverlap, &mut claims, &evidence, &texts);
        assert_eq!(records[0].support_status, "supported");
        assert!(
            records[0]
                .support_rationale
                .ends_with("Full text: results.")
        );
    }

    #[test]
    fn numeric_checks_see_the_whole_paper() {
        let evidence = [record("ev_0001", "Abstract.")];
        let texts = texts("ev_0001", &[section("results", "Power rose by 157.6 W.")]);
        let expanded = with_full_text(&evidence, &texts);
        assert_eq!(expanded[0].snippet, "Abstract.\nPower rose by 157.6 W.");
    }
}
