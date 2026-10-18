//! UCB1 selection for reasoning branches (port of ucb.py).

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// Default exploration constant.
pub const DEFAULT_C: f64 = 1.4;

/// The per-branch statistics UCB needs.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BranchStats {
    /// Mean value estimate of the branch.
    pub q_score: f64,
    /// Times the branch has been visited.
    pub visits: u64,
}

/// UCB1 score with the default exploration constant.
///
/// Unvisited nodes score `f64::INFINITY` so they are always tried first.
/// `parent_visits` of 0 is treated as 1; Python raises on `log(0)` here, but
/// its only caller guards with `max(total_visits, 1)`, so no reachable
/// behaviour changes.
#[must_use]
pub fn ucb_score(q_value: f64, parent_visits: u64, node_visits: u64) -> f64 {
    ucb_score_with(q_value, parent_visits, node_visits, DEFAULT_C)
}

/// UCB1 score with an explicit exploration constant `c`.
#[must_use]
pub fn ucb_score_with(q_value: f64, parent_visits: u64, node_visits: u64, c: f64) -> f64 {
    if node_visits == 0 {
        return f64::INFINITY;
    }
    let parent = parent_visits.max(1) as f64;
    q_value + c * (parent.ln() / node_visits as f64).sqrt()
}

/// Pick the branch with the highest UCB1 score; the first maximum wins.
#[must_use]
pub fn select_best_ucb(
    branches: &IndexMap<String, BranchStats>,
    total_visits: u64,
) -> Option<String> {
    select_best_ucb_with(branches, total_visits, DEFAULT_C)
}

/// Like [`select_best_ucb`] with an explicit exploration constant.
#[must_use]
pub fn select_best_ucb_with(
    branches: &IndexMap<String, BranchStats>,
    total_visits: u64,
    c: f64,
) -> Option<String> {
    let mut best: Option<&String> = None;
    let mut best_score = f64::NEG_INFINITY;
    for (id, stats) in branches {
        let score = ucb_score_with(stats.q_score, total_visits.max(1), stats.visits, c);
        if score > best_score {
            best_score = score;
            best = Some(id);
        }
    }
    best.cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_maximum_wins_on_ties() {
        let branches: IndexMap<String, BranchStats> = [
            (
                "a".to_string(),
                BranchStats {
                    q_score: 0.5,
                    visits: 0,
                },
            ),
            (
                "b".to_string(),
                BranchStats {
                    q_score: 0.9,
                    visits: 0,
                },
            ),
        ]
        .into_iter()
        .collect();
        assert_eq!(select_best_ucb(&branches, 3).as_deref(), Some("a"));
    }

    #[test]
    fn empty_is_none() {
        assert_eq!(select_best_ucb(&IndexMap::new(), 0), None);
    }

    #[test]
    fn zero_parent_visits_does_not_produce_nan() {
        assert!(!ucb_score(0.3, 0, 2).is_nan());
    }
}
