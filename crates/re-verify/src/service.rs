//! Transport-neutral research service (port of `service.py`): runs, search
//! with gap recording, and the full verify-gate-export pipeline.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use re_core::profiles::{classify_research_mode, select_profile};
use re_model::clock::utc_now;
use re_model::ids::short_hex;
use re_model::records::{
    ClaimRecord, EvidenceGap, EvidenceRecord, ResearchRun, RetrievalError, VerificationRecord,
};
use re_retrieval::{AcademicSearchAggregator, RetrievalResult};
use re_store::Store;
use serde_json::{Map, Value, json};

use crate::claims::extract_claims;
use crate::error::VerifyError;
use crate::gate::gate_records;
use crate::runpack::{RunPackInput, export_run_pack, verify_run_pack_attestation};
use crate::verify::{Verifier, verifier_from_name};

/// Python's default search limit inside the pipeline.
pub const PIPELINE_SEARCH_LIMIT: usize = 10;

/// What `scholar_search` found, recorded and reported.
#[derive(Debug, Clone)]
pub struct SearchOutcome {
    /// The evidence, as records.
    pub evidence_records: Vec<EvidenceRecord>,
    /// The search error, if every source failed.
    pub error: Option<RetrievalError>,
    /// Gaps recorded by this search.
    pub gaps: Vec<EvidenceGap>,
}

fn to_value<T: serde::Serialize>(value: &T) -> Value {
    // Records are plain data; serialisation cannot fail.
    serde_json::to_value(value).unwrap_or(Value::Null)
}

impl SearchOutcome {
    /// The JSON tools and the CLI return: Python's payload after
    /// `pop("evidence_records")`.
    #[must_use]
    pub fn to_json(&self) -> Value {
        json!({
            "evidence": self.evidence_records.iter().map(EvidenceRecord::to_dict).collect::<Vec<_>>(),
            "error": self.error.as_ref().map_or(Value::Null, to_value),
            "gaps": self.gaps.iter().map(to_value).collect::<Vec<_>>(),
        })
    }
}

fn payload(pairs: &[(&str, Value)]) -> Map<String, Value> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), v.clone()))
        .collect()
}

/// The verifiable research service.
pub struct VerifiableResearchService {
    store: Store,
    runs_dir: PathBuf,
    search: Arc<AcademicSearchAggregator>,
    verifier: Box<dyn Verifier>,
}

impl VerifiableResearchService {
    /// A service over explicit parts.
    #[must_use]
    pub fn new(
        store: Store,
        runs_dir: impl Into<PathBuf>,
        search: Arc<AcademicSearchAggregator>,
        verifier: Box<dyn Verifier>,
    ) -> Self {
        Self {
            store,
            runs_dir: runs_dir.into(),
            search,
            verifier,
        }
    }

    /// A service configured from the environment: adapters as
    /// `default_adapters`, verifier from `REASONING_ENGINE_VERIFIER`.
    ///
    /// # Errors
    /// An unknown verifier or an HTTP client that cannot be built.
    pub fn from_env(store: Store, runs_dir: impl Into<PathBuf>) -> Result<Self, VerifyError> {
        let verifier =
            verifier_from_name(std::env::var("REASONING_ENGINE_VERIFIER").ok().as_deref())?;
        let search = Arc::new(AcademicSearchAggregator::from_env()?);
        Ok(Self::new(store, runs_dir, search, verifier))
    }

    /// The underlying store.
    #[must_use]
    pub fn store(&self) -> &Store {
        &self.store
    }

    /// Where run packs are written.
    #[must_use]
    pub fn runs_dir(&self) -> &Path {
        &self.runs_dir
    }

    /// Create a run with the selected profile and mode.
    ///
    /// # Errors
    /// An unknown profile or mode, or a store failure.
    pub fn start_run(
        &self,
        query: &str,
        mode: &str,
        profile: &str,
    ) -> Result<ResearchRun, VerifyError> {
        let selected_profile = select_profile(query, profile)?;
        let selected_mode = classify_research_mode(query, mode)?;
        let run = ResearchRun::create(query, selected_profile.name, selected_mode.as_str());
        self.store.save_run(&run)?;
        let id = &run.run_id;
        self.store
            .append_provenance(id, "run_created", &payload(&[("query", query.into())]))?;
        self.store.append_provenance(
            id,
            "profile_selected",
            &payload(&[("profile", selected_profile.name.into())]),
        )?;
        self.store.append_provenance(
            id,
            "mode_selected",
            &payload(&[("mode", selected_mode.as_str().into())]),
        )?;
        Ok(run)
    }

    fn record_gap(
        &self,
        run_id: &str,
        query: &str,
        reason: &str,
    ) -> Result<EvidenceGap, VerifyError> {
        let gap = EvidenceGap {
            gap_id: format!("gap_{run_id}_{}", short_hex(8)),
            run_id: run_id.into(),
            query: query.into(),
            reason: reason.into(),
            created_at: utc_now(),
        };
        self.store.save_gap(&gap)?;
        Ok(gap)
    }

    /// Search, persisting evidence or recording a gap.
    ///
    /// `use_mock_empty` replays Python's `EmptyMockAdapter`: a search that
    /// succeeds with no evidence.
    ///
    /// # Errors
    /// Store failures.
    pub async fn scholar_search(
        &self,
        run_id: &str,
        query: &str,
        limit: usize,
        use_mock_empty: bool,
    ) -> Result<SearchOutcome, VerifyError> {
        self.store.append_provenance(
            run_id,
            "scholar_search_requested",
            &payload(&[("query", query.into()), ("limit", limit.into())]),
        )?;
        let result = if use_mock_empty {
            RetrievalResult::default()
        } else {
            self.search.search(run_id, query, limit).await
        };
        if let Some(error) = result.error {
            let gap = self.record_gap(run_id, query, &error.message)?;
            let Value::Object(details) = to_value(&error) else {
                unreachable!("errors serialise to objects")
            };
            self.store
                .append_provenance(run_id, "evidence_gap_recorded", &details)?;
            return Ok(SearchOutcome {
                evidence_records: Vec::new(),
                error: Some(error),
                gaps: vec![gap],
            });
        }
        if result.evidence.is_empty() {
            let gap = self.record_gap(run_id, query, "No evidence found.")?;
            return Ok(SearchOutcome {
                evidence_records: Vec::new(),
                error: None,
                gaps: vec![gap],
            });
        }
        for item in &result.evidence {
            self.store.save_evidence(item)?;
        }
        self.store.append_provenance(
            run_id,
            "scholar_search_completed",
            &payload(&[("evidence_count", result.evidence.len().into())]),
        )?;
        Ok(SearchOutcome {
            evidence_records: result.evidence,
            error: None,
            gaps: Vec::new(),
        })
    }

    /// Start a run, search, extract and verify claims, gate, export and
    /// attest the run pack.
    ///
    /// # Errors
    /// An unknown profile or mode, or store and filesystem failures.
    pub async fn run_research_pipeline(
        &self,
        query: &str,
        draft: &str,
        mode: &str,
        profile: &str,
        use_mock_empty: bool,
    ) -> Result<Value, VerifyError> {
        let run = self.start_run(query, mode, profile)?;
        let search = self
            .scholar_search(&run.run_id, query, PIPELINE_SEARCH_LIMIT, use_mock_empty)
            .await?;
        let evidence = &search.evidence_records;
        let evidence_dicts: Vec<Value> = evidence.iter().map(EvidenceRecord::to_dict).collect();
        let mut claims = extract_claims(&run.run_id, draft, &run.profile)?;
        let verifications = self.verifier.verify(&mut claims, evidence);
        for claim in &claims {
            self.store.save_claim(claim)?;
        }
        for verification in &verifications {
            self.store.save_verification(&run.run_id, verification)?;
        }
        let gaps = self.store.list_gaps(&run.run_id)?;
        let gate = gate_records(&run.run_id, &claims, &verifications, gaps.len());
        let provenance = self.store.list_provenance(&run.run_id)?;
        let report = build_report(&run, &claims, &verifications, &gate);
        let claim_dicts: Vec<Value> = claims.iter().map(to_value).collect();
        let verification_dicts: Vec<Value> = verifications.iter().map(to_value).collect();
        let output = export_run_pack(
            &self.runs_dir,
            &RunPackInput {
                run: &run,
                evidence: &evidence_dicts,
                gaps: &gaps,
                claims: &claim_dicts,
                verifications: &verification_dicts,
                provenance: &provenance,
                quality_gate: &gate,
                report_markdown: &report,
            },
        )?;
        let attestation = verify_run_pack_attestation(&output);
        let snippets: Vec<&str> = evidence.iter().map(|e| e.snippet.as_str()).collect();
        Ok(json!({
            "run_id": run.run_id,
            "quality_gate": gate,
            "run_pack": output.display().to_string(),
            "attestation": attestation,
            "evidence": evidence_dicts,
            "verified_against_snippets": snippets,
        }))
    }
}

/// The Markdown report: header lines, then one bullet per claim.
#[must_use]
pub fn build_report(
    run: &ResearchRun,
    claims: &[ClaimRecord],
    verifications: &[VerificationRecord],
    gate: &Value,
) -> String {
    let result = gate
        .get("result")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let mut lines = vec![
        format!("# Research Report {}", run.run_id),
        String::new(),
        format!("Query: {}", run.query),
        format!("Profile: {}", run.profile),
        format!("Mode: {}", run.mode),
        format!("Quality gate: {result}"),
        String::new(),
        "## Claims".to_string(),
    ];
    for claim in claims {
        // Last verification per claim wins, as with Python's dict.
        let status = verifications
            .iter()
            .rev()
            .find(|v| v.claim_id == claim.claim_id)
            .map_or(claim.status.as_str(), |v| v.support_status.as_str());
        lines.push(format!("- [{status}] {}", claim.text));
    }
    lines.join("\n") + "\n"
}
