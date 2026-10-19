//! DORA budget allocation and branch selection (port of dora.py).
//!
//! This is the legacy kappa-switch allocator kept for parity. The
//! direction-level allocator from the DORA paper lands in Phase 5 as
//! `dora::directional`.
//!
//! Parity notes (plan section 11): branch maps keep insertion order
//! (`IndexMap`) and sorting is stable, so ties resolve exactly as Python's
//! `sorted(..., reverse=True)` does.

pub mod reestimate;

use std::collections::HashSet;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::types::{Allocation, Strategy};

/// Score spread above which DORA keeps exploring (breadth).
pub const KAPPA_THRESHOLD: f64 = 0.15;
/// Branches scoring below this are sent to reflexion.
pub const REFLEXION_THRESHOLD: f64 = 0.5;
/// Branches scoring below this are pruned in breadth mode.
pub const PRUNE_THRESHOLD: f64 = 0.25;

/// Difficulty below which a single pass is enough.
pub const DIFFICULTY_LOW: f64 = 0.3;
/// Difficulty below which best-of-n is used.
pub const DIFFICULTY_MEDIUM: f64 = 0.5;
/// Difficulty below which beam search is used; above it, forest.
pub const DIFFICULTY_HIGH: f64 = 0.7;
/// Branches kept in depth mode.
pub const DEPTH_MAX_BRANCHES: usize = 2;

/// Compute budget derived from difficulty.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetAllocation {
    /// Search strategy.
    pub strategy: Strategy,
    /// Branches to generate.
    pub total_branches: u32,
    /// Maximum tree depth.
    pub max_depth: u32,
    /// Maximum planner steps.
    pub max_steps: u32,
    /// Token budget per branch.
    pub tokens_per_branch: u32,
    /// Reflexion rounds allowed.
    pub reflexion_rounds: u32,
}

/// Outcome of one planning step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AllocationResult {
    /// Branches to keep working on.
    pub branches_to_continue: Vec<String>,
    /// Branches to send through reflexion.
    pub branches_to_reflect: Vec<String>,
    /// Branches to drop.
    pub branches_to_prune: Vec<String>,
    /// Explore/exploit decision.
    pub allocation: Allocation,
    /// Score spread (max - min).
    pub kappa: f64,
    /// Planner steps left after this decision.
    pub budget_remaining: i64,
}

/// Map a difficulty in `[0, 1]` to a compute budget.
#[must_use]
pub fn allocate_budget(difficulty: f64) -> BudgetAllocation {
    let (strategy, total_branches, max_depth, max_steps, tokens_per_branch, reflexion_rounds) =
        if difficulty < DIFFICULTY_LOW {
            (Strategy::SinglePass, 1, 3, 5, 2000, 0)
        } else if difficulty < DIFFICULTY_MEDIUM {
            (Strategy::BestOfN, 3, 5, 15, 3000, 1)
        } else if difficulty < DIFFICULTY_HIGH {
            (Strategy::BeamSearch, 5, 8, 30, 4000, 2)
        } else {
            (Strategy::Forest, 8, 12, 50, 5000, 3)
        };
    BudgetAllocation {
        strategy,
        total_branches,
        max_depth,
        max_steps,
        tokens_per_branch,
        reflexion_rounds,
    }
}

/// Decide which branches to continue, reflect on or prune.
///
/// `scores` must preserve insertion order; ties are broken by it.
#[must_use]
pub fn select_branches(scores: &IndexMap<String, f64>, budget_remaining: i64) -> AllocationResult {
    let budget_remaining = budget_remaining.max(0);
    if scores.is_empty() || budget_remaining == 0 {
        return AllocationResult {
            branches_to_continue: Vec::new(),
            branches_to_reflect: Vec::new(),
            branches_to_prune: scores.keys().cloned().collect(),
            allocation: Allocation::BudgetExhausted,
            kappa: 0.0,
            budget_remaining,
        };
    }

    let kappa = if scores.len() > 1 {
        let max = scores.values().copied().fold(f64::NEG_INFINITY, f64::max);
        let min = scores.values().copied().fold(f64::INFINITY, f64::min);
        max - min
    } else {
        0.0
    };

    let mut sorted: Vec<(&String, f64)> = scores.iter().map(|(k, v)| (k, *v)).collect();
    // Stable sort, descending: equal scores keep insertion order, as in Python.
    sorted.sort_by(|a, b| b.1.total_cmp(&a.1));

    let mut to_continue = Vec::new();
    let mut to_reflect = Vec::new();
    let mut to_prune = Vec::new();

    let allocation = if kappa > KAPPA_THRESHOLD {
        for (id, score) in &sorted {
            if *score < PRUNE_THRESHOLD {
                to_prune.push((*id).clone());
            } else if *score < REFLEXION_THRESHOLD {
                to_reflect.push((*id).clone());
            } else {
                to_continue.push((*id).clone());
            }
        }
        Allocation::Breadth
    } else {
        let top_n = DEPTH_MAX_BRANCHES.min(sorted.len()).max(1);
        for (i, (id, score)) in sorted.iter().enumerate() {
            if i < top_n {
                to_continue.push((*id).clone());
            } else if *score < REFLEXION_THRESHOLD {
                to_reflect.push((*id).clone());
            } else {
                to_prune.push((*id).clone());
            }
        }
        Allocation::Depth
    };

    let action_budget = usize::try_from(budget_remaining).unwrap_or(usize::MAX);
    if to_continue.len() + to_reflect.len() > action_budget {
        let combined: Vec<String> = to_continue.iter().chain(&to_reflect).cloned().collect();
        let allowed: HashSet<&String> = combined.iter().take(action_budget).collect();
        for id in &combined {
            if !allowed.contains(id) && !to_prune.contains(id) {
                to_prune.push(id.clone());
            }
        }
        to_continue.retain(|id| allowed.contains(id));
        to_reflect.retain(|id| allowed.contains(id));
    }

    let steps_used = i64::try_from(to_continue.len() + to_reflect.len()).unwrap_or(i64::MAX);
    AllocationResult {
        branches_to_continue: to_continue,
        branches_to_reflect: to_reflect,
        branches_to_prune: to_prune,
        allocation,
        kappa,
        budget_remaining: (budget_remaining - steps_used).max(0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn map(pairs: &[(&str, f64)]) -> IndexMap<String, f64> {
        pairs.iter().map(|(k, v)| ((*k).to_string(), *v)).collect()
    }

    #[test]
    fn ties_keep_insertion_order() {
        let r = select_branches(&map(&[("x", 0.4), ("y", 0.4), ("z", 0.4)]), 5);
        assert_eq!(r.allocation, Allocation::Depth);
        assert_eq!(r.branches_to_continue, vec!["x", "y"]);
        assert_eq!(r.branches_to_reflect, vec!["z"]);
    }

    #[test]
    fn negative_budget_is_clamped() {
        let r = select_branches(&map(&[("a", 0.9)]), -3);
        assert_eq!(r.allocation, Allocation::BudgetExhausted);
        assert_eq!(r.budget_remaining, 0);
        assert_eq!(r.branches_to_prune, vec!["a"]);
    }

    proptest! {
        #[test]
        fn outputs_partition_the_input(
            raw in proptest::collection::vec(0.0f64..=1.0, 0..12),
            budget in -5i64..60,
        ) {
            let scores: IndexMap<String, f64> =
                raw.iter().enumerate().map(|(i, s)| (format!("b{i}"), *s)).collect();
            let r = select_branches(&scores, budget);

            let mut seen = HashSet::new();
            for id in r.branches_to_continue.iter()
                .chain(&r.branches_to_reflect)
                .chain(&r.branches_to_prune)
            {
                prop_assert!(seen.insert(id.clone()), "{id} appears twice");
            }
            let keys: HashSet<String> = scores.keys().cloned().collect();
            prop_assert_eq!(seen, keys);
            prop_assert!(r.budget_remaining >= 0);
            if budget <= 0 {
                prop_assert_eq!(r.allocation, Allocation::BudgetExhausted);
            }
        }
    }
}
