//! Domain research profiles and mode classification (port of verifiable/profiles.py).

use std::fmt;
use std::str::FromStr;

use serde::ser::SerializeMap;
use serde::{Deserialize, Serialize, Serializer};

use crate::types::CoreError;

/// Per-profile scoring weights, serialised as a JSON object.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Weights(pub &'static [(&'static str, f64)]);

impl Serialize for Weights {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (key, value) in self.0 {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

/// How a research domain should be searched, cited and checked.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResearchProfile {
    /// Profile identifier.
    pub name: &'static str,
    /// Source types to prefer.
    pub preferred_source_types: &'static [&'static str],
    /// Source types to treat as weak.
    pub weak_source_types: &'static [&'static str],
    /// Minimum independent sources for a claim.
    pub minimum_evidence_count: u32,
    /// `normal` or `fast`.
    pub recency_requirement: &'static str,
    /// Citation style identifier.
    pub citation_style: &'static str,
    /// `medium`, `high` or `maximum`.
    pub claim_strictness: &'static str,
    /// Phrases that make a query high risk in this domain.
    pub high_risk_triggers: &'static [&'static str],
    /// Caveats every report in this domain must carry.
    pub required_caveats: &'static [&'static str],
    /// Quality scoring weights.
    pub scoring_weights: Weights,
}

const BASE_WEIGHTS: Weights = Weights(&[
    ("source_quality", 0.25),
    ("citation_support", 0.30),
    ("recency", 0.15),
    ("contradiction_handling", 0.20),
    ("provenance_completeness", 0.10),
]);

/// All profiles, in the order Python defines them.
pub const PROFILES: &[ResearchProfile] = &[
    ResearchProfile {
        name: "general",
        preferred_source_types: &[
            "peer_reviewed_article",
            "standards_document",
            "book",
            "primary_source",
        ],
        weak_source_types: &["social_media"],
        minimum_evidence_count: 1,
        recency_requirement: "normal",
        citation_style: "harvard_au",
        claim_strictness: "medium",
        high_risk_triggers: &[],
        required_caveats: &[],
        scoring_weights: BASE_WEIGHTS,
    },
    ResearchProfile {
        name: "medicine",
        preferred_source_types: &[
            "peer_reviewed_article",
            "clinical_guideline",
            "systematic_review",
        ],
        weak_source_types: &["blog_post", "social_media"],
        minimum_evidence_count: 2,
        recency_requirement: "fast",
        citation_style: "vancouver",
        claim_strictness: "maximum",
        high_risk_triggers: &["diagnosis", "treatment", "drug", "dose", "clinical"],
        required_caveats: &["Medical findings require professional clinical review."],
        scoring_weights: BASE_WEIGHTS,
    },
    ResearchProfile {
        name: "law",
        preferred_source_types: &[
            "legislation",
            "case_law",
            "regulator_guidance",
            "peer_reviewed_article",
        ],
        weak_source_types: &["blog_post", "social_media"],
        minimum_evidence_count: 2,
        recency_requirement: "fast",
        citation_style: "legal",
        claim_strictness: "maximum",
        high_risk_triggers: &[
            "liability",
            "compliance",
            "contract",
            "statute",
            "jurisdiction",
        ],
        required_caveats: &["Legal findings require jurisdiction-specific professional review."],
        scoring_weights: BASE_WEIGHTS,
    },
    ResearchProfile {
        name: "business",
        preferred_source_types: &["market_report", "financial_filing", "peer_reviewed_article"],
        weak_source_types: &["social_media"],
        minimum_evidence_count: 2,
        recency_requirement: "normal",
        citation_style: "harvard_au",
        claim_strictness: "high",
        high_risk_triggers: &["investment", "valuation", "forecast", "revenue"],
        required_caveats: &[
            "Business and financial conclusions depend on changing market conditions.",
        ],
        scoring_weights: BASE_WEIGHTS,
    },
    ResearchProfile {
        name: "engineering",
        preferred_source_types: &[
            "standards_document",
            "technical_report",
            "peer_reviewed_article",
        ],
        weak_source_types: &["social_media"],
        minimum_evidence_count: 2,
        recency_requirement: "normal",
        citation_style: "ieee",
        claim_strictness: "high",
        high_risk_triggers: &["safety", "failure", "load", "certification"],
        required_caveats: &["Engineering claims require context-specific validation."],
        scoring_weights: BASE_WEIGHTS,
    },
    ResearchProfile {
        name: "science",
        preferred_source_types: &["peer_reviewed_article", "preprint", "dataset"],
        weak_source_types: &["blog_post", "social_media"],
        minimum_evidence_count: 2,
        recency_requirement: "normal",
        citation_style: "harvard_au",
        claim_strictness: "high",
        high_risk_triggers: &["causal", "replication", "statistically significant"],
        required_caveats: &[],
        scoring_weights: BASE_WEIGHTS,
    },
    ResearchProfile {
        name: "humanities",
        preferred_source_types: &["book", "peer_reviewed_article", "primary_source"],
        weak_source_types: &["social_media"],
        minimum_evidence_count: 1,
        recency_requirement: "normal",
        citation_style: "chicago",
        claim_strictness: "medium",
        high_risk_triggers: &["attribution", "translation", "archive"],
        required_caveats: &[],
        scoring_weights: BASE_WEIGHTS,
    },
    ResearchProfile {
        name: "policy",
        preferred_source_types: &[
            "government_report",
            "regulator_guidance",
            "peer_reviewed_article",
        ],
        weak_source_types: &["social_media"],
        minimum_evidence_count: 2,
        recency_requirement: "fast",
        citation_style: "harvard_au",
        claim_strictness: "high",
        high_risk_triggers: &[
            "public health",
            "national security",
            "regulation",
            "election",
        ],
        required_caveats: &["Policy conclusions depend on jurisdiction and timing."],
        scoring_weights: BASE_WEIGHTS,
    },
    ResearchProfile {
        name: "security",
        preferred_source_types: &[
            "peer_reviewed_article",
            "conference_paper",
            "standards_document",
            "vendor_advisory",
        ],
        weak_source_types: &["blog_post", "social_media"],
        minimum_evidence_count: 2,
        recency_requirement: "fast",
        citation_style: "harvard_au",
        claim_strictness: "high",
        high_risk_triggers: &[
            "exploit",
            "vulnerability",
            "malware",
            "credential",
            "prompt injection",
            "mcp",
        ],
        required_caveats: &[
            "Security findings may change quickly as patches and disclosures evolve.",
        ],
        scoring_weights: BASE_WEIGHTS,
    },
    ResearchProfile {
        name: "ai_safety",
        preferred_source_types: &[
            "peer_reviewed_article",
            "technical_report",
            "model_card",
            "standards_document",
        ],
        weak_source_types: &["social_media"],
        minimum_evidence_count: 2,
        recency_requirement: "fast",
        citation_style: "harvard_au",
        claim_strictness: "high",
        high_risk_triggers: &[
            "alignment",
            "eval",
            "dangerous capability",
            "misuse",
            "agent",
        ],
        required_caveats: &[
            "AI safety findings can change quickly as model capabilities and evaluations evolve.",
        ],
        scoring_weights: BASE_WEIGHTS,
    },
];

/// Keywords that select a profile in `auto` mode, checked in this order.
pub const PROFILE_KEYWORDS: &[(&str, &[&str])] = &[
    (
        "security",
        &[
            "security",
            "vulnerability",
            "malware",
            "credential",
            "prompt injection",
            "mcp",
            "exploit",
        ],
    ),
    (
        "ai_safety",
        &[
            "ai safety",
            "alignment",
            "dangerous capability",
            "eval",
            "agent",
        ],
    ),
    (
        "medicine",
        &[
            "medical",
            "clinical",
            "diagnosis",
            "treatment",
            "drug",
            "patient",
        ],
    ),
    (
        "law",
        &[
            "legal",
            "law",
            "contract",
            "liability",
            "compliance",
            "jurisdiction",
        ],
    ),
    (
        "business",
        &["business", "market", "revenue", "investment", "valuation"],
    ),
    (
        "engineering",
        &[
            "engineering",
            "system design",
            "safety-critical",
            "load",
            "certification",
        ],
    ),
    (
        "science",
        &["science", "experiment", "replication", "dataset", "study"],
    ),
    (
        "humanities",
        &["literature", "archive", "translation", "philosophy"],
    ),
    (
        "policy",
        &[
            "policy",
            "regulation",
            "public health",
            "government",
            "election",
        ],
    ),
];

/// Phrases that force `high_stakes` mode.
pub const HIGH_STAKES_TERMS: &[&str] = &[
    "medical",
    "clinical",
    "diagnosis",
    "treatment",
    "legal",
    "liability",
    "financial",
    "investment",
    "safety",
    "vulnerability",
    "exploit",
    "credential",
    "prompt injection",
    "public health",
];

/// Research depth and rigour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResearchMode {
    /// Fast, shallow pass.
    Quick,
    /// Default.
    Standard,
    /// More branches and evidence.
    Deep,
    /// Academic sources first.
    Scholarly,
    /// Audit trail emphasis.
    Audit,
    /// Strictest verification; forced by risky topics.
    HighStakes,
}

impl ResearchMode {
    /// The wire string Python uses.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Quick => "quick",
            Self::Standard => "standard",
            Self::Deep => "deep",
            Self::Scholarly => "scholarly",
            Self::Audit => "audit",
            Self::HighStakes => "high_stakes",
        }
    }
}

impl fmt::Display for ResearchMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ResearchMode {
    type Err = CoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "quick" => Self::Quick,
            "standard" => Self::Standard,
            "deep" => Self::Deep,
            "scholarly" => Self::Scholarly,
            "audit" => Self::Audit,
            "high_stakes" => Self::HighStakes,
            other => return Err(CoreError::UnknownMode(other.to_string())),
        })
    }
}

/// Profiles sorted by name, like Python's `list_profiles`.
#[must_use]
pub fn list_profiles() -> Vec<&'static ResearchProfile> {
    let mut profiles: Vec<_> = PROFILES.iter().collect();
    profiles.sort_by_key(|p| p.name);
    profiles
}

/// Look up a profile by name.
///
/// # Errors
/// [`CoreError::UnknownProfile`] for names outside [`PROFILES`].
pub fn get_profile(name: &str) -> Result<&'static ResearchProfile, CoreError> {
    PROFILES
        .iter()
        .find(|p| p.name == name)
        .ok_or_else(|| CoreError::UnknownProfile(name.to_string()))
}

/// Pick a profile: an explicit name, or keyword matching when `auto`.
///
/// # Errors
/// [`CoreError::UnknownProfile`] when an explicit name is not a profile.
pub fn select_profile(query: &str, requested: &str) -> Result<&'static ResearchProfile, CoreError> {
    if requested != "auto" {
        return get_profile(requested);
    }
    let lowered = query.to_lowercase();
    let name = PROFILE_KEYWORDS
        .iter()
        .find(|(_, keywords)| keywords.iter().any(|kw| lowered.contains(kw)))
        .map_or("general", |(name, _)| *name);
    get_profile(name)
}

/// Escalate to `high_stakes` for risky topics, else keep the requested mode.
///
/// # Errors
/// [`CoreError::UnknownMode`] when `requested` is not a valid mode.
pub fn classify_research_mode(query: &str, requested: &str) -> Result<ResearchMode, CoreError> {
    let mode: ResearchMode = requested.parse()?;
    let lowered = query.to_lowercase();
    if mode == ResearchMode::HighStakes || HIGH_STAKES_TERMS.iter().any(|t| lowered.contains(t)) {
        return Ok(ResearchMode::HighStakes);
    }
    Ok(mode)
}
