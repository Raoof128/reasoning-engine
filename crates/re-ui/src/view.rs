//! View models: everything a template shows, computed in Rust so the
//! templates only print escaped text and plain numbers.

use std::collections::{HashMap, HashSet};

use re_store::audit::ReflectionRow;
use re_store::research::RunRow;
use re_store::sessions::{BranchRow, SessionRow};
use serde_json::Value;

/// Branch statuses with a colour; anything else renders as `unknown`.
const STATUSES: [&str; 4] = ["active", "reflecting", "pruned", "completed"];
/// Verification statuses with a colour.
const SUPPORT: [&str; 6] = [
    "supported",
    "partially_supported",
    "unsupported",
    "contradicted",
    "needs_more_evidence",
    "unverified",
];

const COLUMN: i64 = 150;
const ROW: i64 = 46;
const MARGIN: i64 = 36;
const BAR: i64 = 26;
const CHART_HEIGHT: i64 = 120;

fn known(value: &str, allowed: &[&str]) -> String {
    if allowed.contains(&value) {
        value.to_string()
    } else {
        "unknown".to_string()
    }
}

/// The first 8 characters of an ID.
#[must_use]
pub fn short(id: &str) -> String {
    id.chars().take(8).collect()
}

/// `value` as a link target only when it is an absolute `http(s)` URL, so a
/// stored `javascript:` URL can never become a link.
#[must_use]
pub fn safe_url(value: Option<&str>) -> Option<String> {
    let url = value?.trim();
    let lower = url.to_ascii_lowercase();
    ((lower.starts_with("https://") || lower.starts_with("http://"))
        && !url.chars().any(char::is_control))
    .then(|| url.to_string())
}

fn text(value: &Value, key: &str) -> String {
    match value.get(key) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

fn number(value: &Value, key: &str) -> Option<f64> {
    value.get(key).and_then(Value::as_f64)
}

/// A session in the index list.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionItem {
    /// Full ID (link target).
    pub id: String,
    /// Short ID.
    pub short_id: String,
    /// Question.
    pub query: String,
    /// `active` or `completed`.
    pub status: String,
    /// Difficulty, two decimals.
    pub difficulty: String,
    /// Strategy.
    pub strategy: String,
    /// `remaining / max` steps.
    pub steps: String,
    /// Creation time.
    pub created_at: String,
}

impl From<&SessionRow> for SessionItem {
    fn from(s: &SessionRow) -> Self {
        Self {
            id: s.id.clone(),
            short_id: short(&s.id),
            query: s.query.clone(),
            status: s.status.clone(),
            difficulty: format!("{:.2}", s.difficulty),
            strategy: s.strategy.clone(),
            steps: format!("{} / {}", s.budget_remaining_steps, s.budget_max_steps),
            created_at: s.created_at.clone(),
        }
    }
}

/// A research run in the index list.
#[derive(Debug, Clone, PartialEq)]
pub struct RunItem {
    /// Full ID (link target).
    pub id: String,
    /// Short ID.
    pub short_id: String,
    /// Question.
    pub query: String,
    /// Profile.
    pub profile: String,
    /// Mode.
    pub mode: String,
    /// Lifecycle state.
    pub status: String,
    /// Creation time.
    pub created_at: String,
}

impl From<&RunRow> for RunItem {
    fn from(r: &RunRow) -> Self {
        Self {
            id: r.run_id.clone(),
            short_id: short(&r.run_id),
            query: r.query.clone(),
            profile: r.profile.clone(),
            mode: r.mode.clone(),
            status: r.status.clone(),
            created_at: r.created_at.clone(),
        }
    }
}

/// One node of the branch tree.
#[derive(Debug, Clone, PartialEq)]
pub struct TreeNode {
    /// Centre x.
    pub x: i64,
    /// Centre y.
    pub y: i64,
    /// Label x (right of the node).
    pub label_x: i64,
    /// Short branch ID.
    pub short_id: String,
    /// `q_score`, two decimals.
    pub score: String,
    /// Colour class.
    pub status: String,
}

/// The branch tree as SVG geometry.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Tree {
    /// SVG width.
    pub width: i64,
    /// SVG height.
    pub height: i64,
    /// Nodes in preorder.
    pub nodes: Vec<TreeNode>,
    /// Elbow paths from parent to child (`d` attributes; numbers only).
    pub edges: Vec<String>,
}

/// Lay out `branches` (oldest first) as a left-to-right tree: depth is the
/// column, preorder position the row. Parent cycles and missing parents
/// make a branch a root, so every branch appears exactly once.
#[must_use]
pub fn tree(branches: &[BranchRow]) -> Tree {
    let ids: HashSet<&str> = branches.iter().map(|b| b.id.as_str()).collect();
    let mut children: HashMap<&str, Vec<&BranchRow>> = HashMap::new();
    for b in branches {
        if let Some(parent) = b
            .parent_id
            .as_deref()
            .filter(|p| ids.contains(p) && *p != b.id)
        {
            children.entry(parent).or_default().push(b);
        }
    }
    let mut placed: HashMap<&str, (i64, i64)> = HashMap::new();
    let mut out = Tree::default();
    let mut max_level = 0;
    // Roots first, then anything a cycle kept unreachable.
    let starts = branches
        .iter()
        .filter(|b| {
            b.parent_id
                .as_deref()
                .is_none_or(|p| !ids.contains(p) || p == b.id)
        })
        .chain(branches.iter());
    for start in starts {
        let mut stack = vec![(start, 0_i64)];
        while let Some((branch, level)) = stack.pop() {
            if placed.contains_key(branch.id.as_str()) {
                continue;
            }
            let row = i64::try_from(out.nodes.len()).unwrap_or(i64::MAX);
            let (x, y) = (MARGIN + level * COLUMN, MARGIN + row * ROW);
            placed.insert(&branch.id, (x, y));
            max_level = max_level.max(level);
            if let Some(&(px, py)) = branch.parent_id.as_deref().and_then(|p| placed.get(p)) {
                let mid = i64::midpoint(px, x);
                out.edges
                    .push(format!("M{} {py}H{mid}V{y}H{}", px + 12, x - 12));
            }
            out.nodes.push(TreeNode {
                x,
                y,
                label_x: x + 18,
                short_id: short(&branch.id),
                score: format!("{:.2}", branch.q_score),
                status: known(&branch.status, &STATUSES),
            });
            if let Some(kids) = children.get(branch.id.as_str()) {
                for kid in kids.iter().rev() {
                    stack.push((kid, level + 1));
                }
            }
        }
    }
    let rows = i64::try_from(out.nodes.len()).unwrap_or(0);
    out.width = 2 * MARGIN + max_level * COLUMN + 110;
    out.height = 2 * MARGIN + (rows - 1).max(0) * ROW;
    out
}

/// One bar of the score chart.
#[derive(Debug, Clone, PartialEq)]
pub struct Bar {
    /// Left edge.
    pub x: i64,
    /// Top edge.
    pub y: i64,
    /// Bar height.
    pub height: i64,
    /// Label x (bar centre).
    pub label_x: i64,
    /// 1-based creation order.
    pub index: usize,
    /// Score, two decimals (tooltip).
    pub score: String,
    /// Short branch ID (tooltip).
    pub short_id: String,
    /// Colour class.
    pub status: String,
}

/// `q_score` per branch in creation order, scaled to 0..1.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Chart {
    /// SVG width.
    pub width: i64,
    /// SVG height.
    pub height: i64,
    /// Baseline y.
    pub base: i64,
    /// y of the 1.0 line.
    pub top: i64,
    /// Bars.
    pub bars: Vec<Bar>,
}

/// Scores over the session, one bar per branch in creation order.
#[must_use]
pub fn chart(branches: &[BranchRow]) -> Chart {
    let top = 16;
    let base = top + CHART_HEIGHT;
    let bars: Vec<Bar> = branches
        .iter()
        .enumerate()
        .map(|(i, b)| {
            #[allow(
                clippy::cast_possible_truncation,
                reason = "a clamped score times a small height"
            )]
            let height = (b.q_score.clamp(0.0, 1.0) * CHART_HEIGHT as f64).round() as i64;
            let x = MARGIN + i64::try_from(i).unwrap_or(0) * (BAR + 6);
            Bar {
                x,
                y: base - height,
                height,
                label_x: x + BAR / 2,
                index: i + 1,
                score: format!("{:.2}", b.q_score),
                short_id: short(&b.id),
                status: known(&b.status, &STATUSES),
            }
        })
        .collect();
    let n = i64::try_from(bars.len()).unwrap_or(0);
    Chart {
        width: 2 * MARGIN + n * (BAR + 6),
        height: base + 24,
        base,
        top,
        bars,
    }
}

/// A branch row in the session table.
#[derive(Debug, Clone, PartialEq)]
pub struct BranchItem {
    /// Short ID.
    pub short_id: String,
    /// Short parent ID, or empty.
    pub parent: String,
    /// Depth.
    pub depth: i64,
    /// `q_score`.
    pub score: String,
    /// Advantage.
    pub advantage: String,
    /// Confidence.
    pub confidence: String,
    /// Visits.
    pub visits: i64,
    /// Colour class.
    pub status: String,
    /// Raw status text.
    pub status_text: String,
    /// Sanitised critique.
    pub critique: String,
    /// Trace steps.
    pub steps: Vec<String>,
}

impl From<&BranchRow> for BranchItem {
    fn from(b: &BranchRow) -> Self {
        Self {
            short_id: short(&b.id),
            parent: b.parent_id.as_deref().map(short).unwrap_or_default(),
            depth: b.depth,
            score: format!("{:.3}", b.q_score),
            advantage: format!("{:.3}", b.advantage),
            confidence: format!("{:.2}", b.confidence),
            visits: b.visits,
            status: known(&b.status, &STATUSES),
            status_text: b.status.clone(),
            critique: b.critique.clone(),
            steps: serde_json::from_str(&b.trace).unwrap_or_default(),
        }
    }
}

/// A reflection row.
#[derive(Debug, Clone, PartialEq)]
pub struct ReflectionItem {
    /// Short branch ID.
    pub branch: String,
    /// `before -> after`.
    pub change: String,
    /// Whether the score went up.
    pub improved: bool,
    /// Critique.
    pub critique: String,
    /// Revision.
    pub revision: String,
    /// Time.
    pub created_at: String,
}

impl From<&ReflectionRow> for ReflectionItem {
    fn from(r: &ReflectionRow) -> Self {
        Self {
            branch: short(&r.branch_id),
            change: format!("{:.2} → {:.2}", r.score_before, r.score_after),
            improved: r.score_after > r.score_before,
            critique: r.original_critique.clone(),
            revision: r.revision_summary.clone(),
            created_at: r.created_at.clone(),
        }
    }
}

/// An evidence row.
#[derive(Debug, Clone, PartialEq)]
pub struct EvidenceItem {
    /// Short evidence ID.
    pub short_id: String,
    /// Title.
    pub title: String,
    /// Safe link, if any.
    pub url: Option<String>,
    /// Adapter.
    pub source: String,
    /// Year or empty.
    pub year: String,
    /// Score, three decimals.
    pub score: String,
    /// Risk flags.
    pub risk_flags: Vec<String>,
    /// Snippet.
    pub snippet: String,
}

/// Evidence rows from stored payloads, highest score first.
#[must_use]
pub fn evidence(payloads: &[Value]) -> Vec<EvidenceItem> {
    let mut items: Vec<(f64, EvidenceItem)> = payloads
        .iter()
        .map(|p| {
            let score = number(p, "score").unwrap_or(0.0);
            let doi_url = p
                .get("doi")
                .and_then(Value::as_str)
                .filter(|d| !d.is_empty())
                .map(|d| format!("https://doi.org/{d}"));
            let url = safe_url(p.get("url").and_then(Value::as_str))
                .or_else(|| safe_url(doi_url.as_deref()));
            let item = EvidenceItem {
                short_id: short(&text(p, "evidence_id")),
                title: text(p, "title"),
                url,
                source: text(p, "source_adapter"),
                year: text(p, "year"),
                score: format!("{score:.3}"),
                risk_flags: p
                    .get("risk_flags")
                    .and_then(Value::as_array)
                    .map(|flags| {
                        flags
                            .iter()
                            .filter_map(Value::as_str)
                            .map(str::to_string)
                            .collect()
                    })
                    .unwrap_or_default(),
                snippet: text(p, "snippet"),
            };
            (score, item)
        })
        .collect();
    items.sort_by(|a, b| b.0.total_cmp(&a.0));
    items.into_iter().map(|(_, item)| item).collect()
}

/// A claim with its latest verification.
#[derive(Debug, Clone, PartialEq)]
pub struct ClaimItem {
    /// Claim ID.
    pub id: String,
    /// Claim text.
    pub text: String,
    /// Colour class.
    pub status: String,
    /// Raw status text.
    pub status_text: String,
    /// Confidence or empty.
    pub confidence: String,
    /// Verifier method or empty.
    pub method: String,
    /// Rationale or empty.
    pub rationale: String,
    /// Whether a human should review it.
    pub review: bool,
}

/// Claims joined with their last verification (by `verified_at`).
#[must_use]
pub fn claims(claims: &[Value], verifications: &[Value]) -> Vec<ClaimItem> {
    let mut latest: HashMap<String, &Value> = HashMap::new();
    for v in verifications {
        let id = text(v, "claim_id");
        let newer = latest
            .get(&id)
            .is_none_or(|old| text(v, "verified_at") >= text(old, "verified_at"));
        if newer {
            latest.insert(id, v);
        }
    }
    claims
        .iter()
        .map(|c| {
            let id = text(c, "claim_id");
            let v = latest.get(&id);
            let status_text =
                v.map_or_else(|| "unverified".to_string(), |v| text(v, "support_status"));
            ClaimItem {
                text: text(c, "text"),
                status: known(&status_text, &SUPPORT),
                confidence: v
                    .and_then(|v| number(v, "confidence"))
                    .map(|n| format!("{n:.2}"))
                    .unwrap_or_default(),
                method: v.map(|v| text(v, "method")).unwrap_or_default(),
                rationale: v.map(|v| text(v, "support_rationale")).unwrap_or_default(),
                review: v
                    .and_then(|v| v.get("requires_human_review"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                status_text,
                id,
            }
        })
        .collect()
}

/// A gap or provenance line: `(time, kind, detail)`.
#[derive(Debug, Clone, PartialEq)]
pub struct EventItem {
    /// Time.
    pub time: String,
    /// Event type or gap query.
    pub kind: String,
    /// Remaining fields, compact.
    pub detail: String,
}

/// Evidence gaps.
#[must_use]
pub fn gaps(payloads: &[Value]) -> Vec<EventItem> {
    payloads
        .iter()
        .map(|g| EventItem {
            time: text(g, "created_at"),
            kind: text(g, "query"),
            detail: text(g, "reason"),
        })
        .collect()
}

/// Provenance events, minus the fields every event carries.
#[must_use]
pub fn provenance(payloads: &[Value]) -> Vec<EventItem> {
    payloads
        .iter()
        .map(|e| {
            let detail = e
                .as_object()
                .map(|map| {
                    map.iter()
                        .filter(|(k, _)| {
                            !matches!(k.as_str(), "run_id" | "event_type" | "timestamp")
                        })
                        .map(|(k, v)| match v {
                            Value::String(s) => format!("{k}={s}"),
                            other => format!("{k}={other}"),
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            EventItem {
                time: text(e, "timestamp"),
                kind: text(e, "event_type"),
                detail,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn branch(id: &str, parent: Option<&str>, q: f64, status: &str) -> BranchRow {
        BranchRow {
            id: id.into(),
            session_id: "s".into(),
            parent_id: parent.map(Into::into),
            depth: 0,
            trace: r#"["step one"]"#.into(),
            q_score: q,
            advantage: 0.0,
            critique: String::new(),
            confidence: 0.5,
            visits: 1,
            status: status.into(),
            created_at: "2026-10-19 00:00:00".into(),
        }
    }

    #[test]
    fn only_http_urls_become_links() {
        assert_eq!(
            safe_url(Some(" https://arxiv.org/abs/1 ")),
            Some("https://arxiv.org/abs/1".into())
        );
        assert_eq!(safe_url(Some("HTTP://x.org")), Some("HTTP://x.org".into()));
        for bad in [
            "javascript:alert(1)",
            "data:text/html,x",
            "//x.org",
            "ftp://x",
            "",
        ] {
            assert_eq!(safe_url(Some(bad)), None, "{bad}");
        }
        assert_eq!(safe_url(Some("https://x.org/\nSet-Cookie")), None);
        assert_eq!(safe_url(None), None);
    }

    #[test]
    fn tree_places_children_right_of_parents_in_preorder() {
        let branches = [
            branch("root-a", None, 0.9, "active"),
            branch("root-b", None, 0.4, "pruned"),
            branch("child-a1", Some("root-a"), 0.7, "completed"),
            branch("child-a2", Some("root-a"), 0.2, "weird<status>"),
        ];
        let t = tree(&branches);
        let ids: Vec<&str> = t.nodes.iter().map(|n| n.short_id.as_str()).collect();
        assert_eq!(ids, ["root-a", "child-a1", "child-a2", "root-b"]);
        assert_eq!(t.nodes[1].x, t.nodes[0].x + COLUMN);
        assert_eq!(t.nodes[3].x, t.nodes[0].x);
        assert_eq!(t.edges.len(), 2);
        assert_eq!(
            t.nodes[2].status, "unknown",
            "unknown statuses never reach a class"
        );
        assert!(
            t.edges
                .iter()
                .all(|d| d.chars().all(|c| c.is_ascii_digit() || "MHV -".contains(c)))
        );
        assert!(t.height >= t.nodes[3].y);
    }

    #[test]
    fn tree_survives_cycles_self_parents_and_missing_parents() {
        let branches = [
            branch("p", Some("q"), 0.5, "active"),
            branch("q", Some("p"), 0.5, "active"),
            branch("self", Some("self"), 0.5, "active"),
            branch("orphan", Some("gone"), 0.5, "active"),
        ];
        let t = tree(&branches);
        assert_eq!(t.nodes.len(), 4, "every branch appears exactly once");
        assert_eq!(
            tree(&[]),
            Tree {
                width: 2 * MARGIN + 110,
                height: 2 * MARGIN,
                ..Tree::default()
            }
        );
    }

    #[test]
    fn chart_clamps_scores() {
        let c = chart(&[
            branch("a", None, 1.7, "active"),
            branch("b", None, -0.2, "active"),
        ]);
        assert_eq!(c.bars[0].height, CHART_HEIGHT);
        assert_eq!(c.bars[0].y, c.top);
        assert_eq!(c.bars[1].height, 0);
        assert_eq!(c.bars[1].index, 2);
    }

    #[test]
    fn evidence_sorts_by_score_and_falls_back_to_doi() {
        let rows = evidence(&[
            json!({"evidence_id": "e1", "title": "Low", "score": 0.1, "url": "javascript:x", "doi": "10.1/x", "risk_flags": ["preprint"]}),
            json!({"evidence_id": "e2", "title": "High", "score": 0.9, "url": null, "year": 2024}),
        ]);
        assert_eq!(rows[0].title, "High");
        assert_eq!(rows[0].year, "2024");
        assert_eq!(rows[0].url, None);
        assert_eq!(rows[1].url.as_deref(), Some("https://doi.org/10.1/x"));
        assert_eq!(rows[1].risk_flags, ["preprint"]);
    }

    #[test]
    fn claims_take_the_latest_verification() {
        let items = claims(
            &[
                json!({"claim_id": "c1", "text": "A"}),
                json!({"claim_id": "c2", "text": "B"}),
            ],
            &[
                json!({"claim_id": "c1", "support_status": "unsupported", "verified_at": "2026-01-01T00:00:00Z", "confidence": 0.2}),
                json!({"claim_id": "c1", "support_status": "supported", "verified_at": "2026-01-02T00:00:00Z", "confidence": 0.81, "method": "nli_cross_encoder", "requires_human_review": true}),
            ],
        );
        assert_eq!(items[0].status, "supported");
        assert_eq!(items[0].confidence, "0.81");
        assert!(items[0].review);
        assert_eq!(items[1].status, "unverified");
        assert_eq!(items[1].confidence, "");
    }

    #[test]
    fn provenance_drops_common_fields() {
        let events = provenance(&[
            json!({"run_id": "r", "event_type": "run_created", "timestamp": "t", "query": "q", "n": 2}),
        ]);
        assert_eq!(events[0].kind, "run_created");
        assert_eq!(events[0].detail, "query=q, n=2");
    }
}
