//! Mid-run budget re-estimation (plan Phase 10).
//!
//! A session's budget comes from the query's estimated difficulty. After
//! the first evaluation round the branches themselves say how hard the
//! problem is: wide disagreement between them (kappa, the score spread)
//! and low scores mean a harder problem than the words suggested, and
//! sparse evidence does too. [`reestimate`] blends that signal half and
//! half with the prior estimate and moves the strategy by at most one tier
//! towards the result, returning the change in planner steps. The forest
//! tier is never exceeded and the single-pass tier never undercut.

use serde::{Deserialize, Serialize};

use super::allocate_budget;
use crate::types::Strategy;

/// Tiers in order of effort.
const TIERS: [Strategy; 4] = [
    Strategy::SinglePass,
    Strategy::BestOfN,
    Strategy::BeamSearch,
    Strategy::Forest,
];

/// A kappa at or above this is full disagreement.
pub const KAPPA_SCALE: f64 = 0.5;
/// Results per source at or above this is dense evidence.
pub const DENSE_EVIDENCE: f64 = 5.0;

/// What the first evaluation round showed.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Observed {
    /// Score spread across the scored branches (max - min).
    pub kappa: f64,
    /// Mean branch score.
    pub mean_score: f64,
    /// Evidence found per source searched, when known.
    pub evidence_per_source: Option<f64>,
}

/// The outcome of a re-estimation.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Reestimate {
    /// Difficulty the budget was planned from.
    pub prior: f64,
    /// Difficulty after the first round.
    pub observed: f64,
    /// Strategy before.
    pub from: Strategy,
    /// Strategy after: at most one tier away.
    pub to: Strategy,
    /// Planner steps to add (or, when negative, remove).
    pub step_delta: i64,
}

fn tier(strategy: Strategy) -> usize {
    TIERS.iter().position(|s| *s == strategy).unwrap_or(1)
}

/// The difficulty the first round suggests, blended with `prior`.
#[must_use]
pub fn observed_difficulty(prior: f64, observed: &Observed) -> f64 {
    let disagreement = (observed.kappa / KAPPA_SCALE).clamp(0.0, 1.0);
    let weakness = (1.0 - observed.mean_score).clamp(0.0, 1.0);
    let signal = match observed.evidence_per_source {
        Some(density) => {
            let sparsity = 1.0 - (density / DENSE_EVIDENCE).clamp(0.0, 1.0);
            0.4 * disagreement + 0.4 * weakness + 0.2 * sparsity
        }
        None => 0.5 * disagreement + 0.5 * weakness,
    };
    (0.5 * prior.clamp(0.0, 1.0) + 0.5 * signal).clamp(0.0, 1.0)
}

/// Re-estimate a budget planned from `prior` difficulty.
#[must_use]
pub fn reestimate(prior: f64, observed: &Observed) -> Reestimate {
    let difficulty = observed_difficulty(prior, observed);
    let from = allocate_budget(prior).strategy;
    let wanted = tier(allocate_budget(difficulty).strategy);
    let current = tier(from);
    let next = wanted.clamp(
        current.saturating_sub(1),
        (current + 1).min(TIERS.len() - 1),
    );
    let to = TIERS[next];
    let steps = |strategy: Strategy| {
        let difficulty = match strategy {
            Strategy::SinglePass => 0.0,
            Strategy::BestOfN => super::DIFFICULTY_LOW,
            Strategy::BeamSearch => super::DIFFICULTY_MEDIUM,
            Strategy::Forest => super::DIFFICULTY_HIGH,
        };
        i64::from(allocate_budget(difficulty).max_steps)
    };
    Reestimate {
        prior,
        observed: difficulty,
        from,
        to,
        step_delta: steps(to) - steps(from),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observed(kappa: f64, mean_score: f64, density: Option<f64>) -> Observed {
        Observed {
            kappa,
            mean_score,
            evidence_per_source: density,
        }
    }

    #[test]
    fn disagreement_and_weak_scores_raise_the_budget_one_tier() {
        // Planned as best-of-n (15 steps); the branches disagree badly.
        let r = reestimate(0.35, &observed(0.6, 0.3, Some(0.5)));
        assert_eq!((r.from, r.to), (Strategy::BestOfN, Strategy::BeamSearch));
        assert_eq!(r.step_delta, 15, "30 - 15 steps");
        assert!(r.observed > r.prior);
    }

    #[test]
    fn agreement_and_strong_scores_lower_it_one_tier() {
        let r = reestimate(0.75, &observed(0.02, 0.95, Some(8.0)));
        assert_eq!((r.from, r.to), (Strategy::Forest, Strategy::BeamSearch));
        assert_eq!(r.step_delta, -20, "30 - 50 steps");
    }

    #[test]
    fn never_more_than_one_tier_or_beyond_the_ends() {
        // Single pass with the worst possible round: only to best-of-n.
        let up = reestimate(0.0, &observed(1.0, 0.0, Some(0.0)));
        assert_eq!(up.to, Strategy::BestOfN);
        // Forest stays forest however hard it looks.
        let top = reestimate(0.95, &observed(1.0, 0.0, Some(0.0)));
        assert_eq!((top.to, top.step_delta), (Strategy::Forest, 0));
        // Single pass stays single pass however easy.
        let bottom = reestimate(0.1, &observed(0.0, 1.0, Some(10.0)));
        assert_eq!((bottom.to, bottom.step_delta), (Strategy::SinglePass, 0));
        for prior in [0.0, 0.2, 0.4, 0.6, 0.8, 1.0] {
            for kappa in [0.0, 0.3, 1.0] {
                for mean in [0.0, 0.5, 1.0] {
                    let r = reestimate(prior, &observed(kappa, mean, None));
                    assert!(tier(r.to).abs_diff(tier(r.from)) <= 1);
                    assert!((0.0..=1.0).contains(&r.observed));
                }
            }
        }
    }
}
