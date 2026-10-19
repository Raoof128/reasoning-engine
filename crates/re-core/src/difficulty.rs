//! Heuristic query difficulty estimator (port of difficulty.py).
//!
//! Parity notes (plan section 11):
//! - Tokenisation is `to_lowercase()` split on Python whitespace with punctuation left
//!   attached, so `"implications?"` does not match the keyword `implications`.
//! - Multi-word domain keywords match by substring on the lowercased query.
//! - The four weighted components are summed left to right in `f64`.

use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;

/// Keywords that signal analytical complexity.
pub const COMPLEX_KEYWORDS: &[&str] = &[
    "implications",
    "interaction",
    "heterogeneous",
    "integration",
    "trade-offs",
    "tradeoffs",
    "compare",
    "contrast",
    "analyze",
    "synthesize",
    "relationship",
    "architecture",
    "framework",
    "mechanism",
    "algorithm",
    "optimization",
    "scaling",
    "cross-domain",
    "multi-step",
    "multi-modal",
    "emergent",
];

/// Domain keywords; entries containing a space are matched as phrases.
pub const DOMAIN_KEYWORDS: &[&str] = &[
    "llm",
    "transformer",
    "attention",
    "reinforcement",
    "reward",
    "neural",
    "gradient",
    "backpropagation",
    "embedding",
    "tokenization",
    "mcts",
    "beam search",
    "tree search",
    "chain-of-thought",
    "process reward",
    "reflexion",
    "self-critique",
    "agentic",
    "prm",
    "orm",
    "rlhf",
    "dpo",
    "ppo",
    "grpo",
];

static MULTI_PART_SIGNALS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(and|or|but|however|moreover|furthermore|additionally|versus|vs\.?|compared to|in relation to|how do .+ interact with|what are the implications)\b",
    )
    .expect("valid regex")
});

static SINGLE_WORD_DOMAIN: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    DOMAIN_KEYWORDS
        .iter()
        .copied()
        .filter(|kw| !kw.contains(' '))
        .collect()
});

static MULTI_WORD_DOMAIN: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    DOMAIN_KEYWORDS
        .iter()
        .copied()
        .filter(|kw| kw.contains(' '))
        .collect()
});

static COMPLEX_SET: LazyLock<HashSet<&'static str>> =
    LazyLock::new(|| COMPLEX_KEYWORDS.iter().copied().collect());

/// Estimate how hard a research query is, in `[0, 1]`.
///
/// Weights: 0.25 length, 0.35 keywords, 0.25 multi-part structure,
/// 0.15 question count.
#[must_use]
pub fn estimate_difficulty(query: &str) -> f64 {
    if crate::pystr::py_strip(query).is_empty() {
        return 0.0;
    }
    let query_lower = query.to_lowercase();
    let words: Vec<&str> = query_lower
        .split(crate::pystr::py_is_space)
        .filter(|w| !w.is_empty())
        .collect();
    let word_count = words.len();
    let word_set: HashSet<&str> = words.iter().copied().collect();

    let length_score = (word_count as f64 / 60.0).min(1.0);

    let complex_hits = word_set.intersection(&COMPLEX_SET).count();
    let mut domain_hits = word_set.intersection(&SINGLE_WORD_DOMAIN).count();
    domain_hits += MULTI_WORD_DOMAIN
        .iter()
        .filter(|phrase| query_lower.contains(**phrase))
        .count();
    let keyword_score = ((complex_hits as f64 + domain_hits as f64 * 1.5) / 8.0).min(1.0);

    let multi_part_matches = MULTI_PART_SIGNALS.find_iter(query).count();
    let structure_score = (multi_part_matches as f64 / 3.0).min(1.0);

    let question_count = query.matches('?').count();
    let question_score = (question_count as f64 / 3.0).min(1.0);

    let difficulty =
        0.25 * length_score + 0.35 * keyword_score + 0.25 * structure_score + 0.15 * question_score;
    difficulty.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn empty_and_whitespace_are_zero() {
        assert!(estimate_difficulty("").abs() < f64::EPSILON);
        assert!(estimate_difficulty("   \t\n").abs() < f64::EPSILON);
    }

    #[test]
    fn python_whitespace_splits_words() {
        // Python's `split()` breaks on \x1c; the scores must agree.
        let joined = estimate_difficulty("compare\u{1c}the tradeoffs");
        assert_eq!(
            joined.to_bits(),
            estimate_difficulty("compare the tradeoffs").to_bits()
        );
        assert!(estimate_difficulty("\u{1c}\u{1f}").abs() < f64::EPSILON);
    }

    #[test]
    fn punctuation_stays_attached() {
        // Verified against Python: 0.0479... vs 0.0541...
        assert!(estimate_difficulty("implications") < estimate_difficulty("implications?"));
        let bare = estimate_difficulty("implications");
        let with_keyword_hit = 0.25 * (1.0 / 60.0) + 0.35 * (1.0 / 8.0);
        assert!((bare - with_keyword_hit).abs() < 1e-15);
    }

    const NEUTRAL: &[&str] = &["alpha", "delta", "kappa", "sigma", "omega", "zeta"];

    proptest! {
        #[test]
        fn always_in_unit_interval(q in "\\PC{0,400}") {
            let d = estimate_difficulty(&q);
            prop_assert!((0.0..=1.0).contains(&d));
        }

        #[test]
        fn appending_neutral_words_never_decreases(
            base in "[a-zA-Z ?,.]{0,200}",
            extra in proptest::collection::vec(0..NEUTRAL.len(), 1..40),
        ) {
            let before = estimate_difficulty(&base);
            let mut longer = base.clone();
            for i in extra {
                longer.push(' ');
                longer.push_str(NEUTRAL[i]);
            }
            prop_assert!(estimate_difficulty(&longer) >= before);
        }
    }
}
