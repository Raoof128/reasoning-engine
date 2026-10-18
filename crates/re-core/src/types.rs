//! Shared newtypes, enums and errors.

use serde::{Deserialize, Serialize};

/// Errors raised by pure core functions.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum CoreError {
    /// A score was NaN or outside `[0, 1]`.
    #[error("{field} must be between 0.0 and 1.0, got {value}")]
    ScoreOutOfRange {
        /// Name of the offending field, for error messages.
        field: &'static str,
        /// The rejected value.
        value: f64,
    },
}

/// A score guaranteed to be finite and within `[0, 1]`.
///
/// Serialises as a plain JSON number so wire formats match Python.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "f64", into = "f64")]
pub struct Score(f64);

impl Score {
    /// Build a score, rejecting NaN and values outside `[0, 1]`.
    ///
    /// # Errors
    /// Returns [`CoreError::ScoreOutOfRange`] for NaN or out-of-range values.
    pub fn new(value: f64) -> Result<Self, CoreError> {
        Self::named(value, "score")
    }

    /// Like [`Score::new`] but names the field in the error message.
    ///
    /// # Errors
    /// Returns [`CoreError::ScoreOutOfRange`] for NaN or out-of-range values.
    pub fn named(value: f64, field: &'static str) -> Result<Self, CoreError> {
        if value.is_nan() || !(0.0..=1.0).contains(&value) {
            return Err(CoreError::ScoreOutOfRange { field, value });
        }
        Ok(Self(value))
    }

    /// The inner value.
    #[must_use]
    pub fn get(self) -> f64 {
        self.0
    }
}

impl TryFrom<f64> for Score {
    type Error = CoreError;

    fn try_from(value: f64) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<Score> for f64 {
    fn from(score: Score) -> Self {
        score.0
    }
}

/// Search strategy chosen from query difficulty.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Strategy {
    /// One branch, no reflexion.
    SinglePass,
    /// Three branches, one reflexion round.
    BestOfN,
    /// Five branches, two reflexion rounds.
    BeamSearch,
    /// Eight branches, three reflexion rounds.
    Forest,
}

impl Strategy {
    /// The wire string Python uses.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SinglePass => "single_pass",
            Self::BestOfN => "best_of_n",
            Self::BeamSearch => "beam_search",
            Self::Forest => "forest",
        }
    }
}

/// Lifecycle state of a reasoning branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BranchStatus {
    /// Eligible for further work.
    Active,
    /// Flagged for a reflexion cycle.
    Reflecting,
    /// Dropped by the planner.
    Pruned,
    /// Finished.
    Completed,
}

/// DORA's explore/exploit decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Allocation {
    /// High score variance: keep exploring many branches.
    Breadth,
    /// Low score variance: focus on the top branches.
    Depth,
    /// No branches or no budget left.
    BudgetExhausted,
}

#[cfg(test)]
#[allow(clippy::float_cmp, reason = "exact values are under test")]
mod tests {
    use super::*;

    #[test]
    fn score_accepts_bounds_and_rejects_outside() {
        assert_eq!(Score::new(0.0).unwrap().get(), 0.0);
        assert_eq!(Score::new(1.0).unwrap().get(), 1.0);
        assert!(Score::new(-0.0001).is_err());
        assert!(Score::new(1.0001).is_err());
        assert!(Score::new(f64::NAN).is_err());
        assert!(Score::new(f64::INFINITY).is_err());
    }

    #[test]
    fn score_serde_roundtrip_is_plain_number() {
        let s: Score = serde_json::from_str("0.75").unwrap();
        assert_eq!(serde_json::to_string(&s).unwrap(), "0.75");
        assert!(serde_json::from_str::<Score>("1.5").is_err());
    }

    #[test]
    fn enums_use_python_wire_strings() {
        assert_eq!(
            serde_json::to_string(&Strategy::BestOfN).unwrap(),
            "\"best_of_n\""
        );
        assert_eq!(Strategy::BeamSearch.as_str(), "beam_search");
        assert_eq!(
            serde_json::to_string(&BranchStatus::Reflecting).unwrap(),
            "\"reflecting\""
        );
        assert_eq!(
            serde_json::to_string(&Allocation::BudgetExhausted).unwrap(),
            "\"budget_exhausted\""
        );
    }

    #[test]
    fn error_message_names_field() {
        let err = Score::named(2.0, "q_score").unwrap_err();
        assert_eq!(
            err.to_string(),
            "q_score must be between 0.0 and 1.0, got 2"
        );
    }
}
