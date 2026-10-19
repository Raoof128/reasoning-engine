//! Directional DORA (plan section 6.3): budget allocated across research
//! directions rather than individual branches.
//!
//! 1. A direction is a root branch and all its descendants. Branches do not
//!    record the research angle they came from, so the tree they grew in
//!    stands in for it.
//! 2. Direction quality is the mean of its top two scores, so a direction
//!    with many weak candidates is not favoured for its count.
//! 3. The remaining step budget is split by softmax of quality with
//!    temperature `0.1 + 0.9 * difficulty`: harder queries spread it out.
//! 4. Within a direction the next branch is the UCB1 maximum among active
//!    branches, and progressive widening allows it a new child only while
//!    `children < ceil(C * visits^alpha)` with `C = 1`, `alpha = 0.5`.
//!
//! Directions are returned best first. The legacy `dora::select_branches`
//! is unchanged; this only adds advice.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::ucb::ucb_score;

/// Progressive widening constant `C`.
pub const WIDENING_C: f64 = 1.0;
/// Progressive widening exponent `alpha`.
pub const WIDENING_ALPHA: f64 = 0.5;

/// The parts of a branch the planner reads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BranchInfo {
    /// Branch ID.
    pub id: String,
    /// Parent branch, if any.
    pub parent_id: Option<String>,
    /// Critic score.
    pub q_score: f64,
    /// Times scored.
    pub visits: i64,
    /// `active`, `reflecting`, `pruned` or `completed`.
    pub status: String,
}

/// Advice for one direction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Direction {
    /// The root branch's ID.
    pub direction_id: String,
    /// Branches in the direction, in input order.
    pub branch_ids: Vec<String>,
    /// Mean of the top two scores among live branches.
    pub quality: f64,
    /// Softmax share of the step budget.
    pub share: f64,
    /// Whole steps allocated (largest-remainder rounding).
    pub steps: i64,
    /// The active branch to work on next (UCB1), if any.
    pub next_branch: Option<String>,
    /// Whether `next_branch` may gain a new child (progressive widening).
    pub can_expand: bool,
}

fn root<'a>(id: &'a str, parents: &HashMap<&'a str, Option<&'a str>>) -> &'a str {
    let mut current = id;
    // A bounded walk: a parent cycle cannot hang the planner.
    for _ in 0..parents.len() {
        match parents.get(current).copied().flatten() {
            Some(parent) if parents.contains_key(parent) => current = parent,
            _ => break,
        }
    }
    current
}

fn visits(b: &BranchInfo) -> u64 {
    u64::try_from(b.visits).unwrap_or(0)
}

/// Split `budget` steps by `shares` with largest-remainder rounding; ties
/// go to the earlier direction.
fn allocate(budget: i64, shares: &[f64]) -> Vec<i64> {
    let budget = budget.max(0);
    #[allow(
        clippy::cast_possible_truncation,
        reason = "floor of a share of an i64 budget"
    )]
    let mut steps: Vec<i64> = shares
        .iter()
        .map(|s| (s * budget as f64).floor() as i64)
        .collect();
    let mut order: Vec<usize> = (0..shares.len()).collect();
    order.sort_by(|&a, &b| {
        let ra = shares[a] * budget as f64 - steps[a] as f64;
        let rb = shares[b] * budget as f64 - steps[b] as f64;
        rb.total_cmp(&ra)
    });
    let left = budget - steps.iter().sum::<i64>();
    for &i in order.iter().take(usize::try_from(left).unwrap_or(0)) {
        steps[i] += 1;
    }
    steps
}

/// Plan directions for `branches` (in creation order).
#[must_use]
pub fn plan_directions(
    branches: &[BranchInfo],
    budget_remaining: i64,
    difficulty: f64,
) -> Vec<Direction> {
    let parents: HashMap<&str, Option<&str>> = branches
        .iter()
        .map(|b| (b.id.as_str(), b.parent_id.as_deref()))
        .collect();
    let mut children: HashMap<&str, usize> = HashMap::new();
    for b in branches {
        if let Some(parent) = b.parent_id.as_deref() {
            *children.entry(parent).or_default() += 1;
        }
    }
    // Group by root, keeping first-appearance order.
    let mut groups: Vec<(&str, Vec<&BranchInfo>)> = Vec::new();
    for b in branches {
        let r = root(&b.id, &parents);
        match groups.iter_mut().find(|(id, _)| *id == r) {
            Some((_, members)) => members.push(b),
            None => groups.push((r, vec![b])),
        }
    }
    let groups: Vec<(&str, Vec<&BranchInfo>, f64)> = groups
        .into_iter()
        .filter_map(|(id, members)| {
            let mut scores: Vec<f64> = members
                .iter()
                .filter(|b| b.status != "pruned")
                .map(|b| b.q_score)
                .collect();
            scores.sort_by(|a, b| b.total_cmp(a));
            let top: Vec<f64> = scores.into_iter().take(2).collect();
            (!top.is_empty()).then(|| (id, members, top.iter().sum::<f64>() / top.len() as f64))
        })
        .collect();
    let temperature = 0.1 + 0.9 * difficulty.clamp(0.0, 1.0);
    let max_quality = groups.iter().map(|g| g.2).fold(f64::NEG_INFINITY, f64::max);
    let weights: Vec<f64> = groups
        .iter()
        .map(|g| ((g.2 - max_quality) / temperature).exp())
        .collect();
    let total: f64 = weights.iter().sum();
    let shares: Vec<f64> = weights.iter().map(|w| w / total).collect();
    let steps = allocate(budget_remaining, &shares);

    let mut directions: Vec<Direction> = groups
        .into_iter()
        .zip(shares)
        .zip(steps)
        .map(|(((id, members, quality), share), steps)| {
            let parent_visits: u64 = members.iter().map(|b| visits(b)).sum();
            let next = members
                .iter()
                .filter(|b| b.status == "active")
                .fold(None::<(&BranchInfo, f64)>, |best, b| {
                    let score = ucb_score(b.q_score, parent_visits.max(1), visits(b));
                    match best {
                        Some((_, top)) if top >= score => best,
                        _ => Some((b, score)),
                    }
                })
                .map(|(b, _)| b);
            let can_expand = next.is_some_and(|b| {
                #[allow(clippy::cast_precision_loss, reason = "visit counts are small")]
                let limit = (WIDENING_C * (visits(b) as f64).powf(WIDENING_ALPHA)).ceil();
                (children.get(b.id.as_str()).copied().unwrap_or(0) as f64) < limit
            });
            Direction {
                direction_id: id.to_string(),
                branch_ids: members.iter().map(|b| b.id.clone()).collect(),
                quality,
                share,
                steps,
                next_branch: next.map(|b| b.id.clone()),
                can_expand,
            }
        })
        .collect();
    // Best first. Input order can be arbitrary (branches created in the
    // same second tie-break on random UUIDs), so the output must not
    // depend on it beyond breaking exact ties.
    directions.sort_by(|a, b| b.quality.total_cmp(&a.quality));
    directions
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(id: &str, parent: Option<&str>, q: f64, visits: i64, status: &str) -> BranchInfo {
        BranchInfo {
            id: id.into(),
            parent_id: parent.map(Into::into),
            q_score: q,
            visits,
            status: status.into(),
        }
    }

    #[test]
    fn groups_by_root_and_scores_top_two() {
        let branches = [
            b("a", None, 0.9, 2, "active"),
            b("a1", Some("a"), 0.5, 1, "active"),
            b("a2", Some("a"), 0.1, 1, "active"),
            b("x", None, 0.6, 1, "active"),
            b("x1", Some("x"), 0.95, 1, "pruned"),
        ];
        let plan = plan_directions(&branches, 100, 0.5);
        assert_eq!(plan.len(), 2);
        assert_eq!(plan[0].direction_id, "a");
        assert_eq!(plan[0].branch_ids, ["a", "a1", "a2"]);
        assert!(
            (plan[0].quality - 0.7).abs() < 1e-12,
            "mean of 0.9 and 0.5, not dragged down by 0.1"
        );
        assert!(
            (plan[1].quality - 0.6).abs() < 1e-12,
            "pruned branches do not count"
        );
        assert_eq!(plan.iter().map(|d| d.steps).sum::<i64>(), 100);
        // T = 0.55: shares 0.545 / 0.455.
        assert_eq!((plan[0].steps, plan[1].steps), (55, 45));
        assert!((plan.iter().map(|d| d.share).sum::<f64>() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn harder_queries_spread_the_budget() {
        let branches = [
            b("a", None, 0.9, 1, "active"),
            b("x", None, 0.3, 1, "active"),
        ];
        let easy = plan_directions(&branches, 100, 0.0);
        let hard = plan_directions(&branches, 100, 1.0);
        assert!(easy[0].share > hard[0].share);
        assert!(hard[1].steps > easy[1].steps);
    }

    #[test]
    fn ucb_prefers_unvisited_and_widening_limits_children() {
        let branches = [
            b("a", None, 0.9, 4, "active"),
            b("a1", Some("a"), 0.2, 0, "active"),
            b("a2", Some("a"), 0.8, 1, "active"),
        ];
        let plan = plan_directions(&branches, 5, 0.5);
        assert_eq!(
            plan[0].next_branch.as_deref(),
            Some("a1"),
            "unvisited is explored first"
        );
        assert!(!plan[0].can_expand, "an unvisited node may not widen yet");

        let visited = [
            b("a", None, 0.9, 4, "active"),
            b("a1", Some("a"), 0.2, 3, "pruned"),
        ];
        let plan = plan_directions(&visited, 5, 0.5);
        assert_eq!(plan[0].next_branch.as_deref(), Some("a"));
        assert!(plan[0].can_expand, "1 child < ceil(sqrt(4)) = 2");
    }

    #[test]
    fn output_is_best_first_whatever_the_input_order() {
        let forward = [
            b("x", None, 0.3, 1, "active"),
            b("a", None, 0.9, 1, "active"),
        ];
        let reverse = [forward[1].clone(), forward[0].clone()];
        let ids =
            |plan: Vec<Direction>| plan.into_iter().map(|d| d.direction_id).collect::<Vec<_>>();
        assert_eq!(ids(plan_directions(&forward, 10, 0.5)), ["a", "x"]);
        assert_eq!(ids(plan_directions(&reverse, 10, 0.5)), ["a", "x"]);
    }

    #[test]
    fn edge_cases_are_safe() {
        assert_eq!(plan_directions(&[], 10, 0.5), Vec::new());
        let cyclic = [
            b("p", Some("q"), 0.5, 1, "active"),
            b("q", Some("p"), 0.4, 1, "active"),
        ];
        assert!(
            !plan_directions(&cyclic, 3, 0.5).is_empty(),
            "a parent cycle terminates"
        );
        let pruned = [b("a", None, 0.9, 1, "pruned")];
        assert_eq!(plan_directions(&pruned, 3, 0.5), Vec::new());
        assert_eq!(
            plan_directions(&[b("a", None, 0.9, 1, "active")], -4, 0.5)[0].steps,
            0
        );
    }
}
