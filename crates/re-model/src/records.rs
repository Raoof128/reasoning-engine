//! Verifiable research records. Field order follows the Python dataclasses,
//! because `dataclasses.asdict` order is visible in unsorted JSON output.

use re_core::hashing::sha256_prefixed;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{clock, ids};

/// Errors from record validation.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ModelError {
    /// Status outside [`SUPPORT_STATUSES`].
    #[error("unknown support status: {0}")]
    UnknownStatus(String),
    /// Claim type outside [`CLAIM_TYPES`].
    #[error("unknown claim type: {0}")]
    UnknownClaimType(String),
    /// Error type outside [`RETRIEVAL_ERROR_TYPES`].
    #[error("unknown retrieval error type: {0}")]
    UnknownRetrievalErrorType(String),
}

/// Valid verification outcomes.
pub const SUPPORT_STATUSES: &[&str] = &[
    "supported",
    "partially_supported",
    "contradicted",
    "unsupported",
    "needs_more_evidence",
    "hypothesis",
    "not_verifiable",
    "out_of_scope",
];

/// Valid claim types.
pub const CLAIM_TYPES: &[&str] = &[
    "empirical",
    "definitional",
    "causal",
    "comparative",
    "quantitative",
    "historical",
    "legal",
    "medical",
    "security",
    "policy",
    "predictive",
    "normative",
    "methodological",
    "hypothesis",
];

/// Valid retrieval error categories.
pub const RETRIEVAL_ERROR_TYPES: &[&str] = &[
    "auth_required",
    "rate_limited",
    "unavailable",
    "malformed_response",
    "empty_result",
    "licence_restricted",
    "unsupported_query",
    "adapter_misconfigured",
    // Rust-only: a replayed run asked for a response that was not recorded.
    "replay_miss",
];

/// Check a support status.
///
/// # Errors
/// [`ModelError::UnknownStatus`] for values outside [`SUPPORT_STATUSES`].
pub fn normalize_status(status: &str) -> Result<&str, ModelError> {
    if SUPPORT_STATUSES.contains(&status) {
        Ok(status)
    } else {
        Err(ModelError::UnknownStatus(status.to_string()))
    }
}

/// Check a claim type.
///
/// # Errors
/// [`ModelError::UnknownClaimType`] for values outside [`CLAIM_TYPES`].
pub fn normalize_claim_type(claim_type: &str) -> Result<&str, ModelError> {
    if CLAIM_TYPES.contains(&claim_type) {
        Ok(claim_type)
    } else {
        Err(ModelError::UnknownClaimType(claim_type.to_string()))
    }
}

/// One verifiable research run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResearchRun {
    /// `run_` plus 12 hex characters.
    pub run_id: String,
    /// The research question.
    pub query: String,
    /// Selected profile name.
    pub profile: String,
    /// Selected research mode.
    pub mode: String,
    /// Creation time, `utc_now()` format.
    pub created_at: String,
    /// Lifecycle state; Python defaults it to `active`.
    pub status: String,
}

impl ResearchRun {
    /// Like `ResearchRun.create`: fresh ID, current time, status `active`.
    #[must_use]
    pub fn create(query: &str, profile: &str, mode: &str) -> Self {
        Self {
            run_id: ids::new_run_id(),
            query: query.to_string(),
            profile: profile.to_string(),
            mode: mode.to_string(),
            created_at: clock::utc_now(),
            status: "active".to_string(),
        }
    }
}

/// One retrieved piece of evidence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvidenceRecord {
    /// Evidence identifier, e.g. `ev_0001`.
    pub evidence_id: String,
    /// Owning run.
    pub run_id: String,
    /// Adapter that produced it.
    pub source_adapter: String,
    /// Publication type.
    pub source_type: String,
    /// Title.
    pub title: String,
    /// Author names.
    pub authors: Vec<String>,
    /// Publication year.
    pub year: Option<i64>,
    /// Publisher.
    pub publisher: String,
    /// Journal or conference.
    pub venue: String,
    /// Normalised DOI.
    pub doi: Option<String>,
    /// Landing page.
    pub url: Option<String>,
    /// Retrieval time.
    pub retrieved_at: String,
    /// Query that found it.
    pub query: String,
    /// Rank after aggregation.
    pub rank: i64,
    /// Relevance score.
    pub score: f64,
    /// Abstract or excerpt.
    pub snippet: String,
    /// Licence information.
    pub licence_notes: Option<String>,
    /// Integrity flags such as `retracted`.
    pub risk_flags: Vec<String>,
    /// Adapter-specific extras.
    pub metadata: Map<String, Value>,
    /// Rust-only publication data for source-quality scoring. Never
    /// serialised, so records and run packs stay identical to Python's.
    #[serde(skip)]
    pub hints: QualityHints,
}

/// What a provider says about a work's publication type, retraction and
/// open-access status (plan Phase 9). Adapters fill it from the raw
/// response; [`EvidenceRecord::to_dict`] never includes it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QualityHints {
    /// Raw publication types, e.g. Europe PMC `pubType` values or the
    /// `OpenAlex` work `type`.
    pub publication_types: Vec<String>,
    /// Where it was published, e.g. `OpenAlex` `journal` or `repository`.
    pub venue_type: Option<String>,
    /// Whether the provider marks it retracted.
    pub retracted: Option<bool>,
    /// Whether an open-access copy exists.
    pub open_access: Option<bool>,
    /// A `PubMed` Central identifier (`PMCID`), for Europe PMC full text.
    pub pmcid: Option<String>,
    /// A direct open-access PDF link.
    pub pdf_url: Option<String>,
}

impl EvidenceRecord {
    /// `sha256:` hash of the UTF-8 snippet.
    #[must_use]
    pub fn snippet_hash(&self) -> String {
        sha256_prefixed(self.snippet.as_bytes())
    }

    /// Python's `to_dict`: every field plus `snippet_hash` last.
    #[must_use]
    pub fn to_dict(&self) -> Value {
        // Serialising a struct of strings, numbers and maps cannot fail.
        let mut map = match serde_json::to_value(self) {
            Ok(Value::Object(map)) => map,
            _ => Map::new(),
        };
        map.insert("snippet_hash".into(), Value::String(self.snippet_hash()));
        Value::Object(map)
    }
}

/// A factual claim extracted from a draft.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClaimRecord {
    /// `claim_0001`, ...
    pub claim_id: String,
    /// Owning run.
    pub run_id: String,
    /// Claim text.
    pub text: String,
    /// One of [`CLAIM_TYPES`].
    pub claim_type: String,
    /// Research domain.
    pub domain: String,
    /// `high` or `medium`.
    pub importance: String,
    /// `medium` or `low`.
    pub risk_level: String,
    /// Whether evidence must support it.
    pub requires_citation: bool,
    /// Origin, e.g. `draft`.
    pub created_from: String,
    /// One of [`SUPPORT_STATUSES`].
    pub status: String,
    /// Supporting evidence IDs.
    pub evidence_ids: Vec<String>,
    /// Contradicting evidence IDs.
    pub contradiction_ids: Vec<String>,
    /// Verifier confidence.
    pub confidence: Option<f64>,
    /// Whether a person must review it.
    pub human_review_required: bool,
}

impl ClaimRecord {
    /// Build a claim with Python's defaults (`needs_more_evidence`, no
    /// evidence, no confidence) and validate its type.
    ///
    /// # Errors
    /// [`ModelError::UnknownClaimType`] for an invalid `claim_type`.
    #[allow(clippy::too_many_arguments, reason = "mirrors the Python dataclass")]
    pub fn new(
        claim_id: &str,
        run_id: &str,
        text: &str,
        claim_type: &str,
        domain: &str,
        importance: &str,
        risk_level: &str,
        requires_citation: bool,
        created_from: &str,
    ) -> Result<Self, ModelError> {
        let record = Self {
            claim_id: claim_id.into(),
            run_id: run_id.into(),
            text: text.into(),
            claim_type: claim_type.into(),
            domain: domain.into(),
            importance: importance.into(),
            risk_level: risk_level.into(),
            requires_citation,
            created_from: created_from.into(),
            status: "needs_more_evidence".into(),
            evidence_ids: Vec::new(),
            contradiction_ids: Vec::new(),
            confidence: None,
            human_review_required: false,
        };
        record.validate()?;
        Ok(record)
    }

    /// Re-run Python's `__post_init__` checks after mutating fields.
    ///
    /// # Errors
    /// An unknown claim type or status.
    pub fn validate(&self) -> Result<(), ModelError> {
        normalize_claim_type(&self.claim_type)?;
        normalize_status(&self.status)?;
        Ok(())
    }
}

/// The verifier's verdict on one claim.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VerificationRecord {
    /// `ver_` plus 12 hex characters.
    pub verification_id: String,
    /// Claim verified.
    pub claim_id: String,
    /// Verification method, e.g. `term_overlap_mvp`.
    pub method: String,
    /// Evidence used.
    pub evidence_ids: Vec<String>,
    /// One of [`SUPPORT_STATUSES`].
    pub support_status: String,
    /// Why.
    pub support_rationale: String,
    /// What evidence is missing, if anything.
    pub missing_evidence: Option<String>,
    /// Evidence that contradicts the claim.
    pub contradictory_evidence_ids: Vec<String>,
    /// Verifier confidence.
    pub confidence: f64,
    /// Whether a person must review it.
    pub requires_human_review: bool,
    /// Verification time.
    pub verified_at: String,
}

impl VerificationRecord {
    /// Python's `__post_init__` check.
    ///
    /// # Errors
    /// [`ModelError::UnknownStatus`] for an invalid `support_status`.
    pub fn validate(&self) -> Result<(), ModelError> {
        normalize_status(&self.support_status).map(|_| ())
    }
}

/// A search that came back empty or failed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceGap {
    /// Gap identifier.
    pub gap_id: String,
    /// Owning run.
    pub run_id: String,
    /// Query that found nothing.
    pub query: String,
    /// Why.
    pub reason: String,
    /// When.
    pub created_at: String,
}

/// A structured retrieval failure. Never carries token values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RetrievalError {
    /// One of [`RETRIEVAL_ERROR_TYPES`].
    pub error_type: String,
    /// Human-readable message.
    pub message: String,
    /// Whether retrying may help.
    pub retryable: bool,
    /// Extra context, e.g. `failed_sources`.
    pub metadata: Map<String, Value>,
}

impl RetrievalError {
    /// Build and validate a retrieval error.
    ///
    /// # Errors
    /// [`ModelError::UnknownRetrievalErrorType`] for an invalid `error_type`.
    pub fn new(
        error_type: &str,
        message: &str,
        retryable: bool,
        metadata: Map<String, Value>,
    ) -> Result<Self, ModelError> {
        if !RETRIEVAL_ERROR_TYPES.contains(&error_type) {
            return Err(ModelError::UnknownRetrievalErrorType(
                error_type.to_string(),
            ));
        }
        Ok(Self {
            error_type: error_type.into(),
            message: message.into(),
            retryable,
            metadata,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evidence() -> EvidenceRecord {
        EvidenceRecord {
            hints: QualityHints::default(),
            evidence_id: "ev_0001".into(),
            run_id: "run_001".into(),
            source_adapter: "scholar_gateway".into(),
            source_type: "peer_reviewed_article".into(),
            title: "A useful paper".into(),
            authors: vec!["A. Researcher".into()],
            year: Some(2026),
            publisher: "Wiley".into(),
            venue: "Journal".into(),
            doi: Some("10.1000/example".into()),
            url: Some("https://doi.org/10.1000/example".into()),
            retrieved_at: "2026-06-21T00:00:00Z".into(),
            query: "test query".into(),
            rank: 1,
            score: 0.82,
            snippet: "Important evidence.".into(),
            licence_notes: None,
            risk_flags: vec![],
            metadata: Map::new(),
        }
    }

    // Mirrors tests/test_verifiable_models.py.

    #[test]
    fn evidence_record_hashes_snippet() {
        let payload = evidence().to_dict();
        assert!(
            payload["snippet_hash"]
                .as_str()
                .unwrap()
                .starts_with("sha256:")
        );
        assert_eq!(payload["snippet"], "Important evidence.");
        let keys: Vec<&String> = payload.as_object().unwrap().keys().collect();
        assert_eq!(keys.first().map(|k| k.as_str()), Some("evidence_id"));
        assert_eq!(keys.last().map(|k| k.as_str()), Some("snippet_hash"));
    }

    #[test]
    fn claim_record_defaults_to_needs_more_evidence() {
        let claim = ClaimRecord::new(
            "claim_0001",
            "run_001",
            "Scholar Gateway returns Wiley article metadata.",
            "empirical",
            "general",
            "medium",
            "low",
            true,
            "draft",
        )
        .unwrap();
        assert_eq!(claim.status, "needs_more_evidence");
        assert_eq!(claim.evidence_ids, Vec::<String>::new());
        assert_eq!(
            ClaimRecord::new("c", "r", "t", "vibes", "g", "m", "l", true, "d")
                .unwrap_err()
                .to_string(),
            "unknown claim type: vibes"
        );
    }

    #[test]
    fn retrieval_error_serializes_without_token_values() {
        let mut metadata = Map::new();
        metadata.insert("env_var".into(), "SCHOLAR_GATEWAY_ACCESS_TOKEN".into());
        let error = RetrievalError::new(
            "auth_required",
            "Set SCHOLAR_GATEWAY_ACCESS_TOKEN",
            false,
            metadata,
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(error).unwrap(),
            serde_json::json!({
                "error_type": "auth_required",
                "message": "Set SCHOLAR_GATEWAY_ACCESS_TOKEN",
                "retryable": false,
                "metadata": {"env_var": "SCHOLAR_GATEWAY_ACCESS_TOKEN"},
            })
        );
        assert!(RetrievalError::new("oops", "m", false, Map::new()).is_err());
    }

    #[test]
    fn research_run_has_profile_and_mode() {
        let run = ResearchRun::create("Explain MCP security", "security", "standard");
        assert!(run.run_id.starts_with("run_"));
        assert_eq!(run.query, "Explain MCP security");
        assert_eq!(run.profile, "security");
        assert_eq!(run.mode, "standard");
        assert_eq!(run.status, "active");
    }

    #[test]
    fn normalize_status_rejects_unknown_status() {
        assert_eq!(normalize_status("supported").unwrap(), "supported");
        let err = normalize_status("unknown").unwrap_err().to_string();
        assert!(err.contains("support status"));
    }
}
