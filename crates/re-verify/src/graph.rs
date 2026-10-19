//! Evidence graph, corroboration independence and retraction cascades
//! (`graph` feature, plan Phase 10).
//!
//! Each evidence record with a DOI is looked up in `OpenAlex`
//! ([`re_retrieval::links`]); records become nodes, with a `cites` edge when
//! one record references another and a `shared author` edge when two
//! records have an author in common.
//!
//! - **Independence.** A claim's independence is the number of connected
//!   components among its supporting records (edges between supporters
//!   only). Two papers by the same group, or one citing the other, count
//!   once.
//! - **Retraction cascade.** A record is retracted when its provider says
//!   so. A supported or partly supported claim all of whose supporters are
//!   retracted or cite (directly or through other records) a retracted
//!   record rests on retracted work: it is capped at
//!   `partially_supported`, flagged for review, and listed with the
//!   citation paths in `cascade.json`.
//!
//! Only links between the run's own records are seen; a supporter citing a
//! retracted paper the run never retrieved is not caught.

use std::collections::{BTreeMap, HashSet, VecDeque};
use std::time::{SystemTime, UNIX_EPOCH};

use petgraph::graph::{NodeIndex, UnGraph};
use re_model::records::{ClaimRecord, EvidenceRecord, VerificationRecord};
use re_retrieval::AdapterError;
use re_retrieval::links::{LinkFetcher, WorkLinks, normalise_doi};
use re_store::Store;
use serde::Serialize;

/// How long cached links stay fresh (citations rarely change).
pub const LINKS_TTL_SECS: i64 = 7 * 24 * 60 * 60;

/// An edge between two records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Edge {
    /// The first record references the second.
    Cites,
    /// They share an author.
    SharedAuthor,
}

/// The records of one run and the links between them.
#[derive(Debug, Clone, Default)]
pub struct EvidenceGraph {
    /// Evidence identifiers, in evidence order.
    pub nodes: Vec<String>,
    /// `(from, to, kind)` by node position; `Cites` is directed.
    pub edges: Vec<(usize, usize, Edge)>,
}

impl EvidenceGraph {
    /// Link `evidence` using `links` (matched by DOI).
    #[must_use]
    pub fn build(evidence: &[EvidenceRecord], links: &[WorkLinks]) -> Self {
        let by_doi: BTreeMap<&str, &WorkLinks> =
            links.iter().map(|l| (l.doi.as_str(), l)).collect();
        let found: Vec<Option<&WorkLinks>> = evidence
            .iter()
            .map(|e| {
                e.doi
                    .as_deref()
                    .map(normalise_doi)
                    .and_then(|doi| by_doi.get(doi.as_str()).copied())
            })
            .collect();
        let mut edges = Vec::new();
        for (i, a) in found.iter().enumerate() {
            let Some(a) = a else { continue };
            for (j, b) in found.iter().enumerate() {
                let Some(b) = b else { continue };
                if i == j || a.id == b.id {
                    continue;
                }
                if a.references.contains(&b.id) {
                    edges.push((i, j, Edge::Cites));
                }
                if i < j && a.authors.iter().any(|x| b.authors.contains(x)) {
                    edges.push((i, j, Edge::SharedAuthor));
                }
            }
        }
        Self {
            nodes: evidence.iter().map(|e| e.evidence_id.clone()).collect(),
            edges,
        }
    }

    fn position(&self, id: &str) -> Option<usize> {
        self.nodes.iter().position(|n| n == id)
    }

    /// Connected components among `supporters`, using only the edges
    /// between them. Unknown identifiers count as independent.
    #[must_use]
    pub fn independence(&self, supporters: &[String]) -> usize {
        let mut unique: Vec<&String> = Vec::new();
        for s in supporters {
            if !unique.contains(&s) {
                unique.push(s);
            }
        }
        let mut graph: UnGraph<(), ()> = UnGraph::default();
        let index: Vec<(Option<usize>, NodeIndex)> = unique
            .iter()
            .map(|s| (self.position(s), graph.add_node(())))
            .collect();
        for &(a, b, _) in &self.edges {
            let ends = (
                index.iter().find(|(p, _)| *p == Some(a)),
                index.iter().find(|(p, _)| *p == Some(b)),
            );
            if let (Some((_, x)), Some((_, y))) = ends {
                graph.add_edge(*x, *y, ());
            }
        }
        petgraph::algo::connected_components(&graph)
    }

    /// The shortest citation path from `from` to any of `targets`
    /// (`[from]` when `from` is itself a target).
    #[must_use]
    pub fn path_to<S: std::hash::BuildHasher>(
        &self,
        from: &str,
        targets: &HashSet<String, S>,
    ) -> Option<Vec<String>> {
        let start = self.position(from)?;
        let mut previous: Vec<Option<usize>> = vec![None; self.nodes.len()];
        let mut seen = vec![false; self.nodes.len()];
        let mut queue = VecDeque::from([start]);
        seen[start] = true;
        while let Some(node) = queue.pop_front() {
            if targets.contains(&self.nodes[node]) {
                let mut path = vec![self.nodes[node].clone()];
                let mut at = node;
                while let Some(p) = previous[at] {
                    path.push(self.nodes[p].clone());
                    at = p;
                }
                path.reverse();
                return Some(path);
            }
            for &(a, b, kind) in &self.edges {
                if kind == Edge::Cites && a == node && !seen[b] {
                    seen[b] = true;
                    previous[b] = Some(node);
                    queue.push_back(b);
                }
            }
        }
        None
    }

    /// A Mermaid flowchart of `supporters` and the edges between them.
    #[must_use]
    pub fn mermaid(&self, supporters: &[String], evidence: &[EvidenceRecord]) -> String {
        let label = |id: &str| {
            let title = evidence
                .iter()
                .find(|e| e.evidence_id == id)
                .map_or(id, |e| e.title.as_str());
            let short: String = title.chars().take(60).collect();
            short.replace('"', "#quot;").replace(['[', ']', '\n'], " ")
        };
        let mut lines = vec!["```mermaid".to_string(), "graph LR".to_string()];
        for s in supporters {
            lines.push(format!("  {s}[\"{}\"]", label(s)));
        }
        for &(a, b, kind) in &self.edges {
            let (a, b) = (&self.nodes[a], &self.nodes[b]);
            if supporters.contains(a) && supporters.contains(b) {
                lines.push(match kind {
                    Edge::Cites => format!("  {a} -->|cites| {b}"),
                    Edge::SharedAuthor => format!("  {a} ---|shared author| {b}"),
                });
            }
        }
        lines.push("```".to_string());
        lines.join("\n")
    }
}

/// Evidence identifiers a provider marked retracted.
#[must_use]
pub fn retracted(evidence: &[EvidenceRecord]) -> HashSet<String> {
    evidence
        .iter()
        .filter(|e| {
            e.hints.retracted == Some(true)
                || e.risk_flags
                    .iter()
                    .any(|f| f == re_retrieval::quality::RETRACTED)
        })
        .map(|e| e.evidence_id.clone())
        .collect()
}

/// One claim whose support rests on retracted work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CascadeEntry {
    /// The claim.
    pub claim_id: String,
    /// Its text.
    pub claim: String,
    /// Its supporting records.
    pub supporters: Vec<String>,
    /// The retracted records they rest on.
    pub retracted: Vec<String>,
    /// For each supporter, the citation path to retracted work.
    pub paths: Vec<Vec<String>>,
}

/// Cap claims whose every supporter rests on retracted work, in place, and
/// list them.
pub fn cascade<S: std::hash::BuildHasher>(
    graph: &EvidenceGraph,
    retracted: &HashSet<String, S>,
    claims: &mut [ClaimRecord],
    verifications: &mut [VerificationRecord],
) -> Vec<CascadeEntry> {
    let mut entries = Vec::new();
    if retracted.is_empty() {
        return entries;
    }
    for verification in verifications.iter_mut().filter(|v| {
        matches!(
            v.support_status.as_str(),
            "supported" | "partially_supported"
        ) && !v.evidence_ids.is_empty()
    }) {
        let paths: Option<Vec<Vec<String>>> = verification
            .evidence_ids
            .iter()
            .map(|s| graph.path_to(s, retracted))
            .collect();
        let Some(paths) = paths else { continue };
        let mut sources: Vec<String> = paths.iter().filter_map(|p| p.last().cloned()).collect();
        sources.sort();
        sources.dedup();
        let Some(claim) = claims
            .iter_mut()
            .find(|c| c.claim_id == verification.claim_id)
        else {
            continue;
        };
        verification.support_status = "partially_supported".into();
        verification.support_rationale = format!(
            "{} Its support rests on retracted work ({}).",
            verification.support_rationale,
            sources.join(", ")
        );
        verification.missing_evidence =
            Some("Find support that does not depend on retracted work.".into());
        verification.requires_human_review = true;
        claim.status = "partially_supported".into();
        entries.push(CascadeEntry {
            claim_id: claim.claim_id.clone(),
            claim: claim.text.clone(),
            supporters: verification.evidence_ids.clone(),
            retracted: sources,
            paths,
        });
    }
    entries
}

/// The report's `## Evidence graph`: each claim's independence, with a
/// Mermaid diagram when it has more than one supporter. Empty when no
/// claim has supporters.
#[must_use]
pub fn report_section(
    graph: &EvidenceGraph,
    claims: &[ClaimRecord],
    verifications: &[VerificationRecord],
    evidence: &[EvidenceRecord],
) -> String {
    let mut blocks = Vec::new();
    for claim in claims {
        let Some(verification) = verifications
            .iter()
            .rev()
            .find(|v| v.claim_id == claim.claim_id && !v.evidence_ids.is_empty())
        else {
            continue;
        };
        let supporters = &verification.evidence_ids;
        let independence = graph.independence(supporters);
        let mut block = format!(
            "- {}: {} supporting record{}, {independence} independent",
            claim.text,
            supporters.len(),
            if supporters.len() == 1 { "" } else { "s" }
        );
        if supporters.len() > 1 {
            block.push_str("\n\n");
            block.push_str(&graph.mermaid(supporters, evidence));
        }
        blocks.push(block);
    }
    if blocks.is_empty() {
        return String::new();
    }
    format!("\n## Evidence graph\n\n{}\n", blocks.join("\n\n"))
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// Links for the records with a DOI: from the store's cache where fresh
/// (never while taping), the rest fetched in batches and cached. A fetch
/// failure is returned beside whatever was cached.
pub async fn load_links(
    fetcher: &LinkFetcher,
    store: &Store,
    evidence: &[EvidenceRecord],
) -> (Vec<WorkLinks>, Option<AdapterError>) {
    let use_cache = !fetcher.has_tape();
    let now = now_secs();
    let mut links = Vec::new();
    let mut missing = Vec::new();
    for doi in evidence
        .iter()
        .filter_map(|e| e.doi.as_deref())
        .map(normalise_doi)
    {
        if doi.is_empty()
            || missing.contains(&doi)
            || links.iter().any(|l: &WorkLinks| l.doi == doi)
        {
            continue;
        }
        let cached = use_cache
            .then(|| {
                store
                    .cached_evidence(&format!("links:{doi}"), now)
                    .ok()
                    .flatten()
            })
            .flatten()
            .and_then(|payload| serde_json::from_str::<WorkLinks>(&payload).ok());
        match cached {
            Some(found) => links.push(found),
            None => missing.push(doi),
        }
    }
    if missing.is_empty() {
        return (links, None);
    }
    match fetcher.fetch(&missing).await {
        Ok(new_links) => {
            if use_cache {
                for found in &new_links {
                    if let Ok(payload) = serde_json::to_string(found) {
                        // A cache that cannot be written only costs a refetch.
                        let _ = store.cache_evidence(
                            &format!("links:{}", found.doi),
                            "openalex_links",
                            &found.doi,
                            &payload,
                            LINKS_TTL_SECS,
                            now,
                        );
                    }
                }
            }
            links.extend(new_links);
            (links, None)
        }
        Err(error) => (links, Some(error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use re_retrieval::base::{RecordInput, build_record};

    /// Six records: 1 and 2 share an author; 3 cites 4, which is
    /// retracted; 5 cites 3; 6 stands alone.
    fn six() -> (Vec<EvidenceRecord>, Vec<WorkLinks>) {
        let mut evidence = Vec::new();
        let mut links = Vec::new();
        let spec: [(&[&str], &[&str]); 6] = [
            (&[], &["A1"]),
            (&[], &["A1", "A2"]),
            (&["W4"], &["A3"]),
            (&[], &["A4"]),
            (&["W3"], &["A5"]),
            (&[], &["A6"]),
        ];
        for (i, (refs, authors)) in spec.iter().enumerate() {
            let n = i + 1;
            let mut record = build_record(RecordInput {
                title: format!("Paper \"{n}\""),
                ..RecordInput::new("run", "openalex", "q", n)
            });
            record.evidence_id = format!("ev_{n:04}");
            record.doi = Some(format!("10.1/P{n}"));
            if n == 4 {
                record.hints.retracted = Some(true);
            }
            evidence.push(record);
            links.push(WorkLinks {
                doi: format!("10.1/p{n}"),
                id: format!("W{n}"),
                references: refs.iter().map(|r| (*r).to_string()).collect(),
                authors: authors.iter().map(|a| (*a).to_string()).collect(),
            });
        }
        (evidence, links)
    }

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn edges_follow_citations_and_shared_authors() {
        let (evidence, links) = six();
        let graph = EvidenceGraph::build(&evidence, &links);
        assert_eq!(
            graph.edges,
            [
                (0, 1, Edge::SharedAuthor),
                (2, 3, Edge::Cites),
                (4, 2, Edge::Cites)
            ]
        );
    }

    #[test]
    fn independence_counts_components_among_supporters() {
        let (evidence, links) = six();
        let graph = EvidenceGraph::build(&evidence, &links);
        assert_eq!(graph.independence(&ids(&["ev_0001", "ev_0002"])), 1);
        assert_eq!(graph.independence(&ids(&["ev_0001", "ev_0006"])), 2);
        assert_eq!(
            graph.independence(&ids(&["ev_0001", "ev_0002", "ev_0003", "ev_0006"])),
            3
        );
        assert_eq!(graph.independence(&ids(&["ev_0001", "ev_0001"])), 1);
        assert_eq!(graph.independence(&ids(&["ev_9999", "ev_0006"])), 2);
    }

    fn claim(id: &str) -> ClaimRecord {
        ClaimRecord::new(
            id,
            "run",
            "A claim",
            "empirical",
            "general",
            "medium",
            "low",
            true,
            "d",
        )
        .unwrap()
    }

    fn verification(claim_id: &str, supporters: &[&str]) -> VerificationRecord {
        VerificationRecord {
            verification_id: format!("ver_{claim_id}"),
            claim_id: claim_id.into(),
            method: "m".into(),
            evidence_ids: ids(supporters),
            support_status: "supported".into(),
            support_rationale: "Supported.".into(),
            missing_evidence: None,
            contradictory_evidence_ids: Vec::new(),
            confidence: 0.9,
            requires_human_review: false,
            verified_at: "2026-10-19T00:00:00Z".into(),
        }
    }

    #[test]
    fn claims_resting_only_on_retracted_work_are_capped() {
        let (evidence, links) = six();
        let graph = EvidenceGraph::build(&evidence, &links);
        let gone = retracted(&evidence);
        assert_eq!(gone, HashSet::from(["ev_0004".to_string()]));
        let mut claims = [claim("c1"), claim("c2"), claim("c3")];
        let mut verifications = [
            // 5 -> 3 -> 4 (retracted): capped.
            verification("c1", &["ev_0005"]),
            // 3 rests on 4, but 6 is independent: stands.
            verification("c2", &["ev_0003", "ev_0006"]),
            // Unrelated work: stands.
            verification("c3", &["ev_0001"]),
        ];
        let entries = cascade(&graph, &gone, &mut claims, &mut verifications);
        assert_eq!(
            entries,
            [CascadeEntry {
                claim_id: "c1".into(),
                claim: "A claim".into(),
                supporters: ids(&["ev_0005"]),
                retracted: ids(&["ev_0004"]),
                paths: vec![ids(&["ev_0005", "ev_0003", "ev_0004"])],
            }]
        );
        assert_eq!(verifications[0].support_status, "partially_supported");
        assert!(verifications[0].requires_human_review);
        assert!(
            verifications[0]
                .support_rationale
                .ends_with("rests on retracted work (ev_0004).")
        );
        assert_eq!(claims[0].status, "partially_supported");
        assert_eq!(verifications[1].support_status, "supported");
        assert_eq!(verifications[2].support_status, "supported");
    }

    #[test]
    fn the_report_draws_claims_with_several_supporters() {
        let (evidence, links) = six();
        let graph = EvidenceGraph::build(&evidence, &links);
        let claims = [claim("c1"), claim("c2")];
        let verifications = [
            verification("c1", &["ev_0001", "ev_0002"]),
            verification("c2", &["ev_0006"]),
        ];
        assert_eq!(
            report_section(&graph, &claims, &verifications, &evidence),
            "\n## Evidence graph\n\n\
             - A claim: 2 supporting records, 1 independent\n\n\
             ```mermaid\ngraph LR\n  ev_0001[\"Paper #quot;1#quot;\"]\n  \
             ev_0002[\"Paper #quot;2#quot;\"]\n  ev_0001 ---|shared author| ev_0002\n```\n\n\
             - A claim: 1 supporting record, 1 independent\n"
        );
    }
}
