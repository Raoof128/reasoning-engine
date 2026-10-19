//! A learned process-reward prior (`learn` feature, plan Phase 11).
//!
//! From past sessions the engine knows which branches ended in the
//! consensus set (the top three by score). [`Prior`] predicts that from
//! what is known when a branch is registered, before Claude scores it:
//! the words of its trace (hashed into [`HASH_DIM`] buckets), its depth,
//! its length, how many sources it cites, whether it has a parent, and the
//! session's difficulty. The model is L2-regularised logistic regression
//! fitted by full-batch gradient descent: deterministic, dependency-free
//! and small enough to read.
//!
//! The prediction seeds a new branch's score, blended by [`Prior::weight`]
//! (`min(1, sessions / 200)`), so a prior trained on little data barely
//! moves anything.

use serde::{Deserialize, Serialize};

/// Hashed word buckets.
pub const HASH_DIM: usize = 256;
/// Numeric features after the buckets.
pub const NUMERIC: usize = 5;
/// Fewest training sessions [`Prior::train`] accepts.
pub const MIN_SESSIONS: usize = 30;
/// Sessions at which the prior gets full weight.
pub const FULL_WEIGHT_SESSIONS: f64 = 200.0;
/// Branches whose top-scored rank is below this are in the consensus set.
pub const CONSENSUS_TOP_K: usize = 3;

/// What is known about a branch when it is registered.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BranchFeatures {
    /// The reasoning steps.
    pub trace: Vec<String>,
    /// Tree depth.
    pub depth: i64,
    /// Sources the branch cites.
    pub sources: usize,
    /// Whether it extends another branch.
    pub has_parent: bool,
    /// The session's estimated difficulty.
    pub difficulty: f64,
}

/// One training example.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Example {
    /// The session it came from (for splitting).
    pub session_id: String,
    /// Features.
    pub features: BranchFeatures,
    /// Whether the branch ended in the consensus set.
    pub in_consensus: bool,
}

/// FNV-1a, for stable word hashing across builds and platforms.
fn fnv1a(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// The feature vector: L2-normalised word buckets, then the numeric
/// features scaled to about `[0, 1]`.
#[must_use]
pub fn vectorise(features: &BranchFeatures) -> Vec<f64> {
    let mut x = vec![0.0; HASH_DIM + NUMERIC];
    let text = features.trace.join(" ").to_lowercase();
    let words: Vec<&str> = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= 3)
        .collect();
    for word in &words {
        #[allow(clippy::cast_possible_truncation, reason = "reduced modulo HASH_DIM")]
        let bucket = (fnv1a(word) % HASH_DIM as u64) as usize;
        x[bucket] += 1.0;
    }
    let norm = x[..HASH_DIM].iter().map(|v| v * v).sum::<f64>().sqrt();
    if norm > 0.0 {
        for v in &mut x[..HASH_DIM] {
            *v /= norm;
        }
    }
    #[allow(clippy::cast_precision_loss, reason = "small counts")]
    let numeric = [
        features.difficulty.clamp(0.0, 1.0),
        (features.depth as f64 / 10.0).clamp(0.0, 1.0),
        (features.trace.len() as f64 / 10.0).clamp(0.0, 1.0),
        ((features.sources as f64).ln_1p() / 3.0).clamp(0.0, 1.0),
        f64::from(u8::from(features.has_parent)),
    ];
    x[HASH_DIM..].copy_from_slice(&numeric);
    x
}

fn sigmoid(z: f64) -> f64 {
    1.0 / (1.0 + (-z).exp())
}

/// Mean log-loss of `predictions` against `labels` (probabilities clamped
/// away from 0 and 1).
#[must_use]
pub fn log_loss(predictions: &[f64], labels: &[bool]) -> f64 {
    let n = predictions.len().min(labels.len());
    if n == 0 {
        return 0.0;
    }
    let total: f64 = predictions
        .iter()
        .zip(labels)
        .map(|(p, y)| {
            let p = p.clamp(1e-6, 1.0 - 1e-6);
            if *y { -p.ln() } else { -(1.0 - p).ln() }
        })
        .sum();
    #[allow(clippy::cast_precision_loss, reason = "example counts")]
    let mean = total / n as f64;
    mean
}

/// Held-out evaluation recorded with a trained prior.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Metrics {
    /// Training examples.
    pub train_examples: usize,
    /// Held-out examples.
    pub test_examples: usize,
    /// Held-out log-loss of the prior.
    pub log_loss: f64,
    /// Held-out log-loss of always predicting the training base rate.
    pub base_rate_log_loss: f64,
    /// Held-out accuracy at 0.5.
    pub accuracy: f64,
}

/// A trained prior.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Prior {
    /// Model format.
    pub format: u32,
    /// Weights, [`HASH_DIM`] + [`NUMERIC`].
    pub weights: Vec<f64>,
    /// Intercept.
    pub bias: f64,
    /// Sessions it was trained on.
    pub sessions: usize,
    /// Held-out metrics, when a split was evaluated.
    pub metrics: Option<Metrics>,
}

/// Why a prior could not be trained.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PriorError {
    /// Fewer than [`MIN_SESSIONS`] sessions.
    #[error("need at least {MIN_SESSIONS} completed sessions to train a prior, found {0}")]
    TooFewSessions(usize),
    /// Every example has the same label.
    #[error("every branch has the same consensus label; nothing to learn")]
    OneClass,
}

/// Distinct session ids in order of first appearance.
fn sessions(examples: &[Example]) -> Vec<&str> {
    let mut seen: Vec<&str> = Vec::new();
    for e in examples {
        if !seen.contains(&e.session_id.as_str()) {
            seen.push(&e.session_id);
        }
    }
    seen
}

/// Whether a session is held out: the first hex digit of the SHA-256 of
/// its id is below 4 (about a quarter of sessions).
#[must_use]
pub fn held_out(session_id: &str) -> bool {
    crate::hashing::sha256_prefixed(session_id.as_bytes())
        .trim_start_matches("sha256:")
        .chars()
        .next()
        .and_then(|c| c.to_digit(16))
        .is_some_and(|d| d < 4)
}

impl Prior {
    /// Fit on `examples`.
    ///
    /// # Errors
    /// Fewer than [`MIN_SESSIONS`] sessions, or a single label.
    pub fn fit(examples: &[Example]) -> Result<Self, PriorError> {
        let sessions = sessions(examples).len();
        if sessions < MIN_SESSIONS {
            return Err(PriorError::TooFewSessions(sessions));
        }
        Self::fit_unchecked(examples)
    }

    /// [`Prior::fit`] without the session minimum (the held-out split's
    /// training part may fall below it).
    fn fit_unchecked(examples: &[Example]) -> Result<Self, PriorError> {
        let sessions = sessions(examples).len();
        let positives = examples.iter().filter(|e| e.in_consensus).count();
        if positives == 0 || positives == examples.len() {
            return Err(PriorError::OneClass);
        }
        let xs: Vec<Vec<f64>> = examples.iter().map(|e| vectorise(&e.features)).collect();
        let ys: Vec<f64> = examples
            .iter()
            .map(|e| f64::from(u8::from(e.in_consensus)))
            .collect();
        #[allow(clippy::cast_precision_loss, reason = "example counts")]
        let n = xs.len() as f64;
        let dim = HASH_DIM + NUMERIC;
        let mut weights = vec![0.0; dim];
        let base = positives as f64 / n;
        let mut bias = (base / (1.0 - base)).ln();
        let (rate, lambda) = (0.5, 1e-3);
        for _ in 0..400 {
            let mut gradient = vec![0.0; dim];
            let mut gradient_bias = 0.0;
            for (x, y) in xs.iter().zip(&ys) {
                let z = bias + x.iter().zip(&weights).map(|(a, b)| a * b).sum::<f64>();
                let error = sigmoid(z) - y;
                for (g, v) in gradient.iter_mut().zip(x) {
                    *g += error * v;
                }
                gradient_bias += error;
            }
            for (w, g) in weights.iter_mut().zip(&gradient) {
                *w -= rate * (g / n + lambda * *w);
            }
            bias -= rate * gradient_bias / n;
        }
        Ok(Self {
            format: 1,
            weights,
            bias,
            sessions,
            metrics: None,
        })
    }

    /// Fit on the sessions not [`held_out`], measure on the held-out ones,
    /// then fit again on everything and keep those metrics.
    ///
    /// # Errors
    /// As [`Prior::fit`] (on all examples).
    pub fn train(examples: &[Example]) -> Result<Self, PriorError> {
        let mut full = Self::fit(examples)?;
        let (test, train): (Vec<Example>, Vec<Example>) = examples
            .iter()
            .cloned()
            .partition(|e| held_out(&e.session_id));
        if let (false, Ok(partial)) = (test.is_empty(), Self::fit_unchecked(&train)) {
            let labels: Vec<bool> = test.iter().map(|e| e.in_consensus).collect();
            let predictions: Vec<f64> = test.iter().map(|e| partial.predict(&e.features)).collect();
            #[allow(clippy::cast_precision_loss, reason = "example counts")]
            let base = train.iter().filter(|e| e.in_consensus).count() as f64 / train.len() as f64;
            #[allow(clippy::cast_precision_loss, reason = "example counts")]
            let accuracy = predictions
                .iter()
                .zip(&labels)
                .filter(|(p, y)| (**p >= 0.5) == **y)
                .count() as f64
                / labels.len() as f64;
            full.metrics = Some(Metrics {
                train_examples: train.len(),
                test_examples: test.len(),
                log_loss: log_loss(&predictions, &labels),
                base_rate_log_loss: log_loss(&vec![base; labels.len()], &labels),
                accuracy,
            });
        }
        Ok(full)
    }

    /// Probability that a branch with `features` ends in the consensus set.
    #[must_use]
    pub fn predict(&self, features: &BranchFeatures) -> f64 {
        let x = vectorise(features);
        sigmoid(self.bias + x.iter().zip(&self.weights).map(|(a, b)| a * b).sum::<f64>())
    }

    /// How much to trust the prior: `min(1, sessions / 200)`.
    #[must_use]
    pub fn weight(&self) -> f64 {
        #[allow(clippy::cast_precision_loss, reason = "session counts")]
        let sessions = self.sessions as f64;
        (sessions / FULL_WEIGHT_SESSIONS).min(1.0)
    }

    /// The seed score for a new branch: the prediction times the weight.
    #[must_use]
    pub fn seed(&self, features: &BranchFeatures) -> f64 {
        self.weight() * self.predict(features)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `SplitMix64`, for reproducible synthetic sessions.
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

    const GOOD: [&str; 4] = [
        "verified against primary evidence",
        "replicated in two cohorts",
        "measured effect with confidence interval",
        "checked the original dataset",
    ];
    const WEAK: [&str; 4] = [
        "assume it probably works",
        "anecdotal blog post says",
        "speculate without sources",
        "guess based on intuition",
    ];

    /// `n` sessions of six branches; a branch's chance of reaching the
    /// consensus set rises with grounded wording and cited sources.
    fn synthetic(n: usize, seed: u64) -> Vec<Example> {
        let mut rng = Rng(seed);
        let mut examples = Vec::new();
        for s in 0..n {
            let mut branches: Vec<(f64, BranchFeatures)> = (0..6)
                .map(|_| {
                    let grounded = rng.next() < 0.5;
                    let pool = if grounded { GOOD } else { WEAK };
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    let pick = |r: f64| pool[(r * 4.0) as usize % 4].to_string();
                    let trace = vec![pick(rng.next()), pick(rng.next())];
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    let sources = if grounded {
                        2 + (rng.next() * 3.0) as usize
                    } else {
                        0
                    };
                    let quality = if grounded { 0.7 } else { 0.3 } + 0.4 * (rng.next() - 0.5);
                    (
                        quality,
                        BranchFeatures {
                            trace,
                            depth: 1,
                            sources,
                            has_parent: false,
                            difficulty: 0.5,
                        },
                    )
                })
                .collect();
            branches.sort_by(|a, b| b.0.total_cmp(&a.0));
            for (rank, (_, features)) in branches.into_iter().enumerate() {
                examples.push(Example {
                    session_id: format!("session-{seed}-{s}"),
                    features,
                    in_consensus: rank < CONSENSUS_TOP_K,
                });
            }
        }
        examples
    }

    #[test]
    fn the_prior_beats_the_base_rate_on_held_out_sessions() {
        let prior = Prior::train(&synthetic(50, 11)).unwrap();
        let metrics = prior.metrics.clone().expect("a held-out split");
        assert!(metrics.test_examples > 0);
        assert!(metrics.log_loss < metrics.base_rate_log_loss, "{metrics:?}");
        assert!(metrics.accuracy > 0.6, "{metrics:?}");
        // And on fresh sessions it never saw.
        let fresh = synthetic(20, 99);
        let labels: Vec<bool> = fresh.iter().map(|e| e.in_consensus).collect();
        let predictions: Vec<f64> = fresh.iter().map(|e| prior.predict(&e.features)).collect();
        assert!(log_loss(&predictions, &labels) < log_loss(&vec![0.5; labels.len()], &labels));
        assert!((prior.weight() - 0.25).abs() < 1e-9, "50 of 200 sessions");
    }

    #[test]
    fn too_little_data_or_one_label_is_refused() {
        assert_eq!(
            Prior::train(&synthetic(10, 3)).unwrap_err(),
            PriorError::TooFewSessions(10)
        );
        let mut same = synthetic(40, 5);
        for e in &mut same {
            e.in_consensus = true;
        }
        assert_eq!(Prior::train(&same).unwrap_err(), PriorError::OneClass);
    }

    #[test]
    fn training_is_deterministic_and_seeds_are_bounded() {
        let examples = synthetic(40, 7);
        let a = Prior::train(&examples).unwrap();
        let b = Prior::train(&examples).unwrap();
        assert_eq!(a, b);
        let features = &examples[0].features;
        let seed = a.seed(features);
        assert!((0.0..=a.weight()).contains(&seed));
        let json = serde_json::to_string(&a).unwrap();
        assert_eq!(serde_json::from_str::<Prior>(&json).unwrap(), a);
    }
}
