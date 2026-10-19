//! Split conformal calibration of the NLI tier (`graph` feature, plan
//! Phase 10).
//!
//! The NLI cross-encoder gives each (evidence, claim) pair probabilities for
//! entailment, contradiction and neutral, read as the labels `supported`,
//! `contradicted` and `neutral` (as in `bench`). On labelled pairs the
//! nonconformity score is `1 - p(gold label)`. [`Calibration::fit`] takes
//! the `ceil((n + 1)(1 - alpha)) / n` quantile `q` of those scores; for a
//! new pair the prediction set is every label with `1 - p <= q`, which
//! contains the true label with probability at least `1 - alpha` when the
//! pairs are exchangeable with the calibration set (Angelopoulos & Bates
//! 2023). A verdict whose label is the only one in its set is confident at
//! that coverage; one outside its set is not.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::bench::LABELS;

/// Miscoverage the default calibration targets (90% coverage).
pub const DEFAULT_ALPHA: f64 = 0.1;

/// A fitted calibration, stored as `calibration.json` in the data
/// directory.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Calibration {
    /// Target miscoverage.
    pub alpha: f64,
    /// Nonconformity threshold.
    pub quantile: f64,
    /// Calibration pairs.
    pub n: usize,
    /// The NLI model the scores came from.
    pub model: String,
}

/// Probabilities in [`LABELS`] order.
pub type Probabilities = [f64; 3];

impl Calibration {
    /// Fit on `probabilities` with `gold` label indices into [`LABELS`].
    /// `None` without pairs.
    #[must_use]
    pub fn fit(
        probabilities: &[Probabilities],
        gold: &[usize],
        alpha: f64,
        model: &str,
    ) -> Option<Self> {
        let mut scores: Vec<f64> = probabilities
            .iter()
            .zip(gold)
            .map(|(p, g)| 1.0 - p[(*g).min(2)])
            .collect();
        if scores.is_empty() {
            return None;
        }
        scores.sort_by(f64::total_cmp);
        let n = scores.len();
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss,
            reason = "a rank between 1 and n + 1"
        )]
        let rank = ((n as f64 + 1.0) * (1.0 - alpha)).ceil().max(1.0) as usize;
        let quantile = scores.get(rank - 1).copied().unwrap_or(1.0);
        Some(Self {
            alpha,
            quantile,
            n,
            model: model.to_string(),
        })
    }

    /// Target coverage, `1 - alpha`.
    #[must_use]
    pub fn coverage_level(&self) -> f64 {
        1.0 - self.alpha
    }

    /// The labels whose nonconformity is within the threshold.
    #[must_use]
    pub fn prediction_set(&self, probabilities: &Probabilities) -> Vec<&'static str> {
        LABELS
            .iter()
            .zip(probabilities)
            .filter(|(_, p)| 1.0 - **p <= self.quantile)
            .map(|(label, _)| *label)
            .collect()
    }

    /// Share of pairs whose gold label is in their prediction set.
    #[must_use]
    pub fn coverage(&self, probabilities: &[Probabilities], gold: &[usize]) -> f64 {
        let total = probabilities.len().min(gold.len());
        if total == 0 {
            return 0.0;
        }
        let covered = probabilities
            .iter()
            .zip(gold)
            .filter(|(p, g)| self.prediction_set(p).contains(&LABELS[(**g).min(2)]))
            .count();
        #[allow(clippy::cast_precision_loss, reason = "counts of labelled pairs")]
        let share = covered as f64 / total as f64;
        share
    }

    /// Read a stored calibration.
    ///
    /// # Errors
    /// An unreadable or invalid file.
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Store this calibration.
    ///
    /// # Errors
    /// Filesystem failures.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(path, json + "\n")
    }
}

/// A deterministic split of `ids` into (calibration, held-out) indices:
/// a pair is held out when the first hex digit of its id's SHA-256 is
/// odd.
#[must_use]
pub fn split(ids: &[&str]) -> (Vec<usize>, Vec<usize>) {
    let mut calibration = Vec::new();
    let mut held_out = Vec::new();
    for (index, id) in ids.iter().enumerate() {
        let hash = re_core::hashing::sha256_prefixed(id.as_bytes());
        let odd = hash
            .trim_start_matches("sha256:")
            .chars()
            .next()
            .and_then(|c| c.to_digit(16))
            .is_some_and(|d| d % 2 == 1);
        if odd {
            held_out.push(index);
        } else {
            calibration.push(index);
        }
    }
    (calibration, held_out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `SplitMix64`, for reproducible synthetic data.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> f64 {
            self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            #[allow(clippy::cast_precision_loss, reason = "53 random bits")]
            let unit = (z >> 11) as f64 / (1u64 << 53) as f64;
            unit
        }
    }

    /// Pairs whose gold label is drawn from (a sharpened copy of) their own
    /// probabilities: a model that is informative but not perfect.
    fn synthetic(rng: &mut Rng, n: usize) -> (Vec<Probabilities>, Vec<usize>) {
        let mut probabilities = Vec::with_capacity(n);
        let mut gold = Vec::with_capacity(n);
        for _ in 0..n {
            let raw = [rng.next().powi(3), rng.next().powi(3), rng.next().powi(3)];
            let total: f64 = raw.iter().sum::<f64>() + 1e-9;
            let p = raw.map(|x| x / total);
            let draw = rng.next();
            let label = if draw < p[0] {
                0
            } else if draw < p[0] + p[1] {
                1
            } else {
                2
            };
            probabilities.push(p);
            gold.push(label);
        }
        (probabilities, gold)
    }

    #[test]
    fn held_out_coverage_is_within_five_points_of_the_target() {
        let mut rng = Rng(7);
        for alpha in [0.1, 0.2] {
            let (cal_p, cal_g) = synthetic(&mut rng, 1000);
            let (test_p, test_g) = synthetic(&mut rng, 2000);
            let fitted = Calibration::fit(&cal_p, &cal_g, alpha, "synthetic").unwrap();
            let coverage = fitted.coverage(&test_p, &test_g);
            assert!(
                (coverage - (1.0 - alpha)).abs() <= 0.05,
                "alpha {alpha}: coverage {coverage}"
            );
        }
    }

    #[test]
    fn the_quantile_and_sets_follow_the_definition() {
        // Scores 1 - p(gold): 0.1, 0.2, ..., 0.9.
        let probabilities: Vec<Probabilities> = (1..=9)
            .map(|i| {
                let p = 1.0 - f64::from(i) / 10.0;
                [p, (1.0 - p) / 2.0, (1.0 - p) / 2.0]
            })
            .collect();
        let gold = vec![0; 9];
        // ceil(10 * 0.8) = 8th smallest score.
        let fitted = Calibration::fit(&probabilities, &gold, 0.2, "m").unwrap();
        assert!((fitted.quantile - 0.8).abs() < 1e-9);
        assert_eq!(fitted.prediction_set(&[0.9, 0.05, 0.05]), ["supported"]);
        assert_eq!(
            fitted.prediction_set(&[0.5, 0.3, 0.2]),
            ["supported", "contradicted", "neutral"]
        );
        // Too few pairs for the rank: every label is in the set.
        let tiny = Calibration::fit(&probabilities[..2], &gold[..2], 0.1, "m").unwrap();
        assert!((tiny.quantile - 1.0).abs() < f64::EPSILON);
        assert!(Calibration::fit(&[], &[], 0.1, "m").is_none());
    }

    #[test]
    fn calibrations_round_trip_and_splits_are_stable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/calibration.json");
        let fitted = Calibration {
            alpha: 0.1,
            quantile: 0.42,
            n: 70,
            model: "nli".into(),
        };
        fitted.save(&path).unwrap();
        assert_eq!(Calibration::load(&path).unwrap(), fitted);
        let ids = ["a", "b", "c", "d", "e", "f", "g", "h"];
        let (calibration, held_out) = split(&ids);
        assert_eq!(calibration.len() + held_out.len(), ids.len());
        assert_eq!(split(&ids), (calibration, held_out));
    }
}
