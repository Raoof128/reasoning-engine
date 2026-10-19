//! Research angle planning and evidence-gap questions (port of research.py).
//!
//! Deliberate divergence: Python collects query words into a `set`, so when
//! a query hits two or more domain hints the angle order depends on
//! `PYTHONHASHSEED` (22 different orders across 64 seeds for one query).
//! This port uses first-occurrence order, which is one of Python's possible
//! outputs and is identical to Python whenever Python itself is stable.

pub mod negate;

use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::difficulty::estimate_difficulty;
use crate::types::CoreError;
use crate::validation::{MAX_QUERY_CHARS, MAX_TRACE_STEP_CHARS, validate_text};

/// Every research angle and its objective, in priority order.
pub const ANGLE_LIBRARY: &[(&str, &str)] = &[
    (
        "conceptual foundations",
        "Define core concepts, assumptions, and terminology.",
    ),
    (
        "empirical evidence",
        "Find measurements, benchmarks, replications, and negative results.",
    ),
    (
        "mechanisms",
        "Explain causal mechanisms, algorithms, or system internals.",
    ),
    (
        "comparative analysis",
        "Compare alternatives, baselines, and trade-offs.",
    ),
    (
        "limitations and failure modes",
        "Identify boundary conditions, risks, and known failures.",
    ),
    (
        "implementation practice",
        "Translate findings into design or operational guidance.",
    ),
    (
        "recent developments",
        "Look for current work, releases, and active debates.",
    ),
    (
        "contrarian perspective",
        "Test whether the dominant claim is overstated or incomplete.",
    ),
    (
        "security and abuse cases",
        "Review misuse, adversarial, privacy, and safety angles.",
    ),
    (
        "evaluation methodology",
        "Define metrics, datasets, and validation criteria.",
    ),
];

/// Query words that pull a specific angle to the front.
pub const DOMAIN_HINTS: &[(&str, &str)] = &[
    ("security", "security and abuse cases"),
    ("mcp", "security and abuse cases"),
    ("benchmark", "evaluation methodology"),
    ("eval", "evaluation methodology"),
    ("evaluation", "evaluation methodology"),
    ("compare", "comparative analysis"),
    ("vs", "comparative analysis"),
    ("recent", "recent developments"),
    ("latest", "recent developments"),
    ("implementation", "implementation practice"),
    ("deploy", "implementation practice"),
];

/// Starter questions, assigned round-robin by angle position.
pub const QUESTION_STARTERS: &[&str] = &[
    "What is the strongest evidence for this claim?",
    "What would change the conclusion?",
    "Which sources are primary, current, and independently corroborated?",
    "Which assumptions are unresolved?",
];

const HARD_QUERY_ANGLES: &[&str] = &[
    "conceptual foundations",
    "empirical evidence",
    "comparative analysis",
    "limitations and failure modes",
    "evaluation methodology",
];
const EASY_QUERY_ANGLES: &[&str] = &[
    "conceptual foundations",
    "empirical evidence",
    "implementation practice",
];

static WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[a-z0-9-]+").expect("valid regex"));

/// One planned research angle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResearchAngle {
    /// Angle name from [`ANGLE_LIBRARY`].
    pub name: String,
    /// What the angle should establish.
    pub objective: String,
    /// 1-based priority.
    pub priority: u32,
    /// Two questions to start the investigation.
    pub starter_questions: Vec<String>,
}

/// Verification questions for one draft claim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceGap {
    /// 1-based claim position.
    pub claim_id: u32,
    /// The trimmed claim.
    pub claim: String,
    /// Questions a verifier should answer.
    pub questions: Vec<String>,
    /// Evidence bar for the claim.
    pub minimum_evidence: String,
    /// The trimmed query, for context.
    pub query_context: String,
}

/// Case-insensitive de-duplication that keeps the first spelling seen.
#[must_use]
pub fn unique_ordered<'a>(values: impl IntoIterator<Item = &'a str>) -> Vec<&'a str> {
    let mut seen = HashSet::new();
    values
        .into_iter()
        .filter(|v| seen.insert(v.to_lowercase()))
        .collect()
}

fn objective(name: &str) -> &'static str {
    ANGLE_LIBRARY
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, o)| *o)
        .expect("every selected angle comes from ANGLE_LIBRARY")
}

fn hint(word: &str) -> Option<&'static str> {
    DOMAIN_HINTS
        .iter()
        .find(|(w, _)| *w == word)
        .map(|(_, a)| *a)
}

/// Choose up to `max_angles` research angles for `query`.
///
/// # Errors
/// Returns a validation error when the trimmed query is empty or longer than
/// [`MAX_QUERY_CHARS`].
pub fn plan_research_angles(
    query: &str,
    max_angles: usize,
) -> Result<Vec<ResearchAngle>, CoreError> {
    let query = validate_text(query, "query", MAX_QUERY_CHARS)?;
    let lowered = query.to_lowercase();

    let mut seen_words = HashSet::new();
    let mut selected: Vec<&str> = WORD
        .find_iter(&lowered)
        .map(|m| m.as_str())
        .filter(|w| seen_words.insert(*w))
        .filter_map(hint)
        .collect();

    if estimate_difficulty(&query) >= 0.5 {
        selected.extend(HARD_QUERY_ANGLES);
    } else {
        selected.extend(EASY_QUERY_ANGLES);
    }
    selected.extend(ANGLE_LIBRARY.iter().map(|(name, _)| *name));

    Ok(unique_ordered(selected)
        .into_iter()
        .take(max_angles)
        .enumerate()
        .zip(1u32..)
        .map(|((index, name), priority)| ResearchAngle {
            name: name.to_string(),
            objective: objective(name).to_string(),
            priority,
            starter_questions: vec![
                QUESTION_STARTERS[index % QUESTION_STARTERS.len()].to_string(),
                format!("What does the {name} angle add that other paths might miss?"),
            ],
        })
        .collect())
}

/// The adversarial angle's name.
pub const ADVERSARIAL: &str = "adversarial";

/// Difficulty from which DORA plans a beam search or a forest
/// ([`crate::dora::DIFFICULTY_MEDIUM`]).
const ADVERSARIAL_DIFFICULTY: f64 = crate::dora::DIFFICULTY_MEDIUM;

/// The angle that looks for evidence against `query`, its starter
/// questions the [`negate::negated_queries`].
#[must_use]
pub fn adversarial_angle(query: &str, priority: u32) -> ResearchAngle {
    ResearchAngle {
        name: ADVERSARIAL.into(),
        objective: "Look for evidence against the claim: null effects, failed replications \
            and stated limitations."
            .into(),
        priority,
        starter_questions: negate::negated_queries(query),
    }
}

/// [`plan_research_angles`], plus the adversarial angle (plan Phase 10):
/// when `adversarial` is set and the query is hard enough for a beam
/// search or forest, the last of the `max_angles` angles is
/// [`adversarial_angle`].
///
/// # Errors
/// As [`plan_research_angles`].
pub fn plan_research_angles_with(
    query: &str,
    max_angles: usize,
    adversarial: bool,
) -> Result<Vec<ResearchAngle>, CoreError> {
    let hard = estimate_difficulty(query.trim()) >= ADVERSARIAL_DIFFICULTY;
    if !adversarial || !hard || max_angles < 2 {
        return plan_research_angles(query, max_angles);
    }
    let mut angles = plan_research_angles(query, max_angles - 1)?;
    let priority = u32::try_from(angles.len() + 1).unwrap_or(u32::MAX);
    angles.push(adversarial_angle(query.trim(), priority));
    Ok(angles)
}

/// Build verification questions for each draft claim.
///
/// # Errors
/// Returns a validation error for an empty or over-long query or claim; claim
/// errors name the zero-based position, e.g. `claims[1] must not be empty`.
pub fn evidence_gap_questions(
    query: &str,
    claims: &[String],
) -> Result<Vec<EvidenceGap>, CoreError> {
    let query = validate_text(query, "query", MAX_QUERY_CHARS)?;
    claims
        .iter()
        .enumerate()
        .map(|(i, claim)| {
            let claim = validate_text(claim, &format!("claims[{i}]"), MAX_TRACE_STEP_CHARS)?;
            let id = i + 1;
            Ok(EvidenceGap {
                claim_id: u32::try_from(id).unwrap_or(u32::MAX),
                claim,
                questions: vec![
                    format!("What primary source directly supports claim {id} for this query?"),
                    "Is there a newer source or replication that contradicts it?".to_string(),
                    "What citation would let a reader independently verify the claim?".to_string(),
                    "Does the source prove the exact claim, or only a weaker related point?"
                        .to_string(),
                ],
                minimum_evidence: "At least one primary source plus one independent corroborating source for important claims.".to_string(),
                query_context: query.clone(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hints_follow_first_occurrence_order() {
        let names: Vec<String> = plan_research_angles("latest compare security", 3)
            .unwrap()
            .into_iter()
            .map(|a| a.name)
            .collect();
        assert_eq!(
            names,
            [
                "recent developments",
                "comparative analysis",
                "security and abuse cases"
            ]
        );
    }

    #[test]
    fn unique_ordered_is_case_insensitive() {
        assert_eq!(unique_ordered(["A", "b", "a", "B", "c"]), ["A", "b", "c"]);
    }
}

#[cfg(test)]
mod adversarial_tests {
    use super::*;

    /// The hardest golden query (difficulty 0.758, a forest).
    const HARD: &str = "Compare and contrast DPO versus PPO versus GRPO for RLHF; what are the \
        implications for reward hacking and how do they interact with process reward models?";

    #[test]
    fn hard_queries_end_with_the_adversarial_angle() {
        assert!(estimate_difficulty(HARD) >= ADVERSARIAL_DIFFICULTY);
        let legacy = plan_research_angles(HARD, 6).unwrap();
        let angles = plan_research_angles_with(HARD, 6, true).unwrap();
        assert_eq!(angles.len(), 6);
        assert_eq!(angles[..5], legacy[..5], "the first angles are unchanged");
        assert_eq!(angles[5].name, ADVERSARIAL);
        assert_eq!(angles[5].priority, 6);
        assert_eq!(angles[5].starter_questions, negate::negated_queries(HARD));
    }

    #[test]
    fn otherwise_the_legacy_angles_are_returned() {
        let easy = "what is a reward model";
        assert_eq!(
            plan_research_angles_with(easy, 6, true).unwrap(),
            plan_research_angles(easy, 6).unwrap(),
            "easy queries plan no adversarial angle"
        );
        assert_eq!(
            plan_research_angles_with(HARD, 6, false).unwrap(),
            plan_research_angles(HARD, 6).unwrap(),
            "off unless asked"
        );
        assert_eq!(
            plan_research_angles_with(HARD, 1, true).unwrap(),
            plan_research_angles(HARD, 1).unwrap()
        );
        assert!(plan_research_angles_with("  ", 6, true).is_err());
    }
}
