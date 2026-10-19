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
use re_retrieval::tape::Tape;
use re_retrieval::{AcademicSearchAggregator, RetrievalResult};
use re_store::Store;
use serde_json::{Map, Value, json};

use crate::claims::extract_claims;
use crate::error::VerifyError;
use crate::gate::gate_records;
use crate::runpack::{
    AttestationSigner, ReplayFiles, RunPackInput, export_run_pack, verify_run_pack_attestation,
};
use crate::verify::{Verifier, load_verifier};

/// `REASONING_ENGINE_MODELS_DIR`, or `~/.reasoning-engine/models`.
#[must_use]
pub fn default_models_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("REASONING_ENGINE_MODELS_DIR").filter(|d| !d.is_empty()) {
        return PathBuf::from(dir);
    }
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from);
    home.unwrap_or_default()
        .join(".reasoning-engine")
        .join("models")
}

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

/// What a recorded run keeps so `reasoning-engine replay` can re-run it
/// (plan Phase 7).
#[derive(Debug, Clone)]
pub struct Recorder {
    /// The tape the search client records to.
    pub tape: Arc<Tape>,
    /// Clock, seed, sources and verifier; the pipeline adds its own inputs
    /// and writes the lot to `replay.json`.
    pub settings: Map<String, Value>,
}

/// The verifiable research service.
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent opt-in pipeline stages"
)]
pub struct VerifiableResearchService {
    store: Store,
    runs_dir: PathBuf,
    search: Arc<AcademicSearchAggregator>,
    verifier: Box<dyn Verifier>,
    /// The `nli` tier `high_stakes` runs use, loaded on first need.
    high_stakes: tokio::sync::OnceCell<Option<Box<dyn Verifier>>>,
    models_dir: PathBuf,
    offline: bool,
    recorder: Option<Recorder>,
    signer: Option<Arc<dyn AttestationSigner>>,
    ledger: bool,
    quality: bool,
    #[cfg(feature = "fulltext")]
    fulltext: Option<re_retrieval::fulltext::FullTextFetcher>,
    #[cfg(feature = "graph")]
    counter_search: bool,
    #[cfg(feature = "graph")]
    calibration: Option<crate::calibration::Calibration>,
}

/// Records whose full text a pipeline run fetches, best ranked first.
#[cfg(feature = "fulltext")]
pub const FULL_TEXT_LIMIT: usize = 5;

/// Print the high-stakes fallback warning once per process.
static HIGH_STAKES_WARNED: std::sync::Once = std::sync::Once::new();

fn warn_high_stakes(reason: &str) {
    HIGH_STAKES_WARNED.call_once(|| {
        eprintln!("warning: high_stakes runs use the nli verifier, but {reason}; using the configured tier");
    });
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
            high_stakes: tokio::sync::OnceCell::new(),
            models_dir: default_models_dir(),
            offline: std::env::var("REASONING_ENGINE_OFFLINE").is_ok_and(|v| v == "1"),
            recorder: None,
            signer: None,
            ledger: false,
            quality: false,
            #[cfg(feature = "fulltext")]
            fulltext: None,
            #[cfg(feature = "graph")]
            counter_search: false,
            #[cfg(feature = "graph")]
            calibration: None,
        }
    }

    /// Attach conformal prediction sets from `calibration` to each verdict
    /// the tier can score (plan Phase 10; see [`crate::calibration`]).
    #[cfg(feature = "graph")]
    #[must_use]
    pub fn with_calibration(mut self, calibration: crate::calibration::Calibration) -> Self {
        self.calibration = Some(calibration);
        self
    }

    /// Record each verdict's conformal prediction set in a `calibration`
    /// provenance event: the premise is the first cited (or, failing that,
    /// contradicting) record's snippet. Nothing is recorded without a
    /// calibration or a tier that gives probabilities.
    #[cfg(feature = "graph")]
    fn calibrate(
        &self,
        run_id: &str,
        verifier: &dyn Verifier,
        claims: &[ClaimRecord],
        verifications: &[VerificationRecord],
        evidence: &[EvidenceRecord],
    ) -> Result<(), VerifyError> {
        let Some(calibration) = &self.calibration else {
            return Ok(());
        };
        let mut sets = Vec::new();
        for verification in verifications {
            let premise = verification
                .evidence_ids
                .iter()
                .chain(&verification.contradictory_evidence_ids)
                .find_map(|id| evidence.iter().find(|e| &e.evidence_id == id));
            let claim = claims.iter().find(|c| c.claim_id == verification.claim_id);
            let (Some(premise), Some(claim)) = (premise, claim) else {
                continue;
            };
            let Some(probabilities) = verifier.probabilities(&premise.snippet, &claim.text) else {
                continue;
            };
            let set = calibration.prediction_set(&probabilities);
            let label = crate::bench::label_of(&verification.support_status);
            sets.push(json!({
                "verification_id": verification.verification_id,
                "label": label,
                "prediction_set": set,
                "in_prediction_set": set.contains(&label),
            }));
        }
        if sets.is_empty() {
            return Ok(());
        }
        self.store.append_provenance(
            run_id,
            "calibration",
            &payload(&[
                ("coverage_level", calibration.coverage_level().into()),
                ("quantile", calibration.quantile.into()),
                ("model", calibration.model.clone().into()),
                ("verifications", Value::from(sets)),
            ]),
        )?;
        Ok(())
    }

    /// Search for contradictions of supported claims after verification,
    /// when the verifier can judge contradiction (plan Phase 10; see
    /// [`crate::counter`]).
    #[cfg(feature = "graph")]
    #[must_use]
    pub fn with_counter_search(mut self) -> Self {
        self.counter_search = true;
        self
    }

    /// Verify claims against open-access full text fetched with `fetcher`
    /// for the best [`FULL_TEXT_LIMIT`] citable records (plan Phase 9).
    #[cfg(feature = "fulltext")]
    #[must_use]
    pub fn with_full_text(mut self, fetcher: re_retrieval::fulltext::FullTextFetcher) -> Self {
        self.fulltext = Some(fetcher);
        self
    }

    /// Fetch and chunk the full text of the first [`FULL_TEXT_LIMIT`]
    /// records with an open-access source, concurrently, and record what
    /// was found in a `full_text` provenance event. Failures are recorded
    /// and the record keeps its snippet.
    #[cfg(feature = "fulltext")]
    async fn full_texts(
        &self,
        run_id: &str,
        evidence: &[EvidenceRecord],
    ) -> Result<std::collections::BTreeMap<String, Vec<crate::chunks::Chunk>>, VerifyError> {
        let mut texts = std::collections::BTreeMap::new();
        let Some(fetcher) = &self.fulltext else {
            return Ok(texts);
        };
        let wanted: Vec<&EvidenceRecord> = evidence
            .iter()
            .filter(|e| !fetcher.candidates(&e.hints).is_empty())
            .take(FULL_TEXT_LIMIT)
            .collect();
        if wanted.is_empty() {
            return Ok(texts);
        }
        let mut jobs = tokio::task::JoinSet::new();
        for (index, record) in wanted.iter().enumerate() {
            let fetcher = fetcher.clone();
            let hints = record.hints.clone();
            jobs.spawn(async move { (index, fetcher.fetch(&hints).await) });
        }
        let mut outcomes = Vec::with_capacity(wanted.len());
        while let Some(joined) = jobs.join_next().await {
            if let Ok(outcome) = joined {
                outcomes.push(outcome);
            }
        }
        outcomes.sort_by_key(|(index, _)| *index);
        let mut documents = Vec::with_capacity(outcomes.len());
        for (index, result) in outcomes {
            let id = &wanted[index].evidence_id;
            documents.push(match result {
                Ok(Some(document)) => {
                    let chunks = crate::chunks::chunk(&document.sections);
                    let sections: Vec<&str> =
                        document.sections.iter().map(|s| s.label.as_str()).collect();
                    let summary = json!({
                        "evidence_id": id,
                        "format": document.format,
                        "url": document.url,
                        "sections": sections,
                        "chunks": chunks.len(),
                    });
                    texts.insert(id.clone(), chunks);
                    summary
                }
                Ok(None) => json!({"evidence_id": id, "error_type": "empty_result"}),
                Err(error) => json!({
                    "evidence_id": id,
                    "error_type": error.error_type,
                    "message": error.message,
                }),
            });
        }
        self.store.append_provenance(
            run_id,
            "full_text",
            &payload(&[("documents", Value::from(documents))]),
        )?;
        Ok(texts)
    }

    /// Score sources by the run profile's preferred and weak publication
    /// types, flag retractions, and never let retracted work support a
    /// claim (plan Phase 9).
    #[must_use]
    pub fn with_quality(mut self) -> Self {
        self.quality = true;
        self
    }

    /// Record each search's cost (`retrieval_cost`) and each pipeline's
    /// totals (`cost_ledger`) in provenance.
    #[must_use]
    pub fn with_ledger(mut self) -> Self {
        self.ledger = true;
        self
    }

    /// Sign every exported run pack with `signer` and append it to the
    /// database's transparency log.
    #[must_use]
    pub fn with_signer(mut self, signer: Arc<dyn AttestationSigner>) -> Self {
        self.signer = Some(signer);
        self
    }

    /// Record pipeline runs for replay: their run packs gain `replay.json`
    /// and the recorded responses from `recorder.tape`.
    #[must_use]
    pub fn with_recorder(mut self, recorder: Recorder) -> Self {
        self.recorder = Some(recorder);
        self
    }

    /// Where model tiers look for (and download) their models.
    #[must_use]
    pub fn with_models(mut self, models_dir: impl Into<PathBuf>, offline: bool) -> Self {
        self.models_dir = models_dir.into();
        self.offline = offline;
        self
    }

    /// The configured verifier, or `nli` for `high_stakes` runs when this
    /// build and its models allow it (plan decision 1).
    async fn verifier_for(&self, mode: &str) -> &dyn Verifier {
        if mode != "high_stakes" || self.verifier.method() == "nli_cross_encoder" {
            return self.verifier.as_ref();
        }
        let loaded = self
            .high_stakes
            .get_or_init(|| async {
                if !cfg!(feature = "semantic") {
                    warn_high_stakes("this build lacks the semantic feature");
                    return None;
                }
                match load_verifier(Some("nli"), &self.models_dir, self.offline).await {
                    Ok(verifier) => Some(verifier),
                    Err(error) => {
                        warn_high_stakes(&error.to_string());
                        None
                    }
                }
            })
            .await;
        loaded.as_deref().unwrap_or(self.verifier.as_ref())
    }

    /// A service configured from the environment: adapters as
    /// `default_adapters`, verifier from `REASONING_ENGINE_VERIFIER`, models
    /// from `REASONING_ENGINE_MODELS_DIR`.
    ///
    /// # Errors
    /// An unknown verifier, a model that cannot load, or an HTTP client that
    /// cannot be built.
    pub async fn from_env(store: Store, runs_dir: impl Into<PathBuf>) -> Result<Self, VerifyError> {
        let name = std::env::var("REASONING_ENGINE_VERIFIER").ok();
        let offline = std::env::var("REASONING_ENGINE_OFFLINE").is_ok_and(|v| v == "1");
        let models_dir = default_models_dir();
        let verifier = load_verifier(name.as_deref(), &models_dir, offline).await?;
        let search = Arc::new(AcademicSearchAggregator::from_env()?);
        Ok(Self::new(store, runs_dir, search, verifier).with_models(models_dir, offline))
    }

    /// The search aggregator (shared, for background prefetch).
    #[must_use]
    pub fn search_aggregator(&self) -> Arc<AcademicSearchAggregator> {
        Arc::clone(&self.search)
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
        let started = std::time::Instant::now();
        let mut result = if use_mock_empty {
            RetrievalResult::default()
        } else {
            self.search.search(run_id, query, limit).await
        };
        if self.quality && result.error.is_none() {
            let profile = self
                .store
                .get_run(run_id)
                .ok()
                .and_then(|run| re_core::profiles::get_profile(&run.profile).ok());
            if let Some(profile) = profile {
                re_retrieval::quality::apply(
                    &mut result.evidence,
                    profile.preferred_source_types,
                    profile.weak_source_types,
                );
            }
        }
        if self.ledger {
            let returned: usize = result
                .evidence
                .iter()
                .map(|e| e.to_dict().to_string().len())
                .sum();
            let cost = crate::ledger::search_cost(&result.diagnostics, returned, started.elapsed());
            self.store
                .append_provenance(run_id, "retrieval_cost", &cost)?;
        }
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

    /// Search for contradictions of the supported claims when enabled and
    /// the tier can judge contradiction, recording a `counter_search`
    /// provenance event.
    #[cfg(feature = "graph")]
    async fn counter_search(
        &self,
        run_id: &str,
        verifier: &dyn Verifier,
        claims: &mut [ClaimRecord],
        verifications: &mut [VerificationRecord],
    ) -> Result<Vec<crate::counter::Finding>, VerifyError> {
        if !self.counter_search || !verifier.can_contradict() {
            return Ok(Vec::new());
        }
        let findings =
            crate::counter::search(&self.search, run_id, verifier, claims, verifications).await;
        self.store.append_provenance(
            run_id,
            "counter_search",
            &payload(&[("searches", to_value(&findings))]),
        )?;
        Ok(findings)
    }

    /// Verify `claims` against `citable` evidence (and its full text, when
    /// fetched), then apply the numeric check.
    #[cfg_attr(
        not(feature = "fulltext"),
        allow(
            clippy::unused_async,
            clippy::unused_async_trait_impl,
            reason = "fetches full text with fulltext"
        )
    )]
    async fn verify_claims(
        &self,
        run_id: &str,
        verifier: &dyn Verifier,
        claims: &mut [ClaimRecord],
        citable: &[EvidenceRecord],
    ) -> Result<Vec<VerificationRecord>, VerifyError> {
        #[cfg(feature = "fulltext")]
        let texts = self.full_texts(run_id, citable).await?;
        #[cfg(feature = "fulltext")]
        #[cfg_attr(not(feature = "trust"), allow(unused_mut))]
        let mut verifications = crate::chunks::verify(verifier, claims, citable, &texts);
        #[cfg(not(feature = "fulltext"))]
        let _ = run_id;
        #[cfg(not(feature = "fulltext"))]
        #[cfg_attr(not(feature = "trust"), allow(unused_mut))]
        let mut verifications = verifier.verify(claims, citable);
        // A quantitative claim cannot be supported by evidence without its
        // number (plan section 6.6), anywhere in the paper when the full
        // text was read.
        #[cfg(all(feature = "trust", feature = "fulltext"))]
        crate::numeric::enforce(
            claims,
            &mut verifications,
            &crate::chunks::with_full_text(citable, &texts),
        );
        #[cfg(all(feature = "trust", not(feature = "fulltext")))]
        crate::numeric::enforce(claims, &mut verifications, citable);
        Ok(verifications)
    }

    /// `replay.json` and the recordings, when recording: the recorder's
    /// settings plus the pipeline's own `inputs`.
    fn replay_files(&self, inputs: &[(&str, Value)]) -> Option<ReplayFiles> {
        self.recorder.as_ref().map(|recorder| {
            let mut settings = recorder.settings.clone();
            for (key, value) in inputs {
                settings.insert((*key).to_string(), value.clone());
            }
            ReplayFiles {
                inputs: Value::Object(settings),
                recordings: recorder.tape.rendered(),
            }
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
        let verifier = self.verifier_for(&run.mode).await;
        // Retracted work stays in the ledger but may not support a claim.
        let citable: Vec<EvidenceRecord> = if self.quality {
            evidence
                .iter()
                .filter(|e| re_retrieval::quality::citable(e))
                .cloned()
                .collect()
        } else {
            evidence.clone()
        };
        #[cfg_attr(not(feature = "graph"), allow(unused_mut))]
        let mut verifications = self
            .verify_claims(&run.run_id, verifier, &mut claims, &citable)
            .await?;
        #[cfg(feature = "graph")]
        let findings = self
            .counter_search(&run.run_id, verifier, &mut claims, &mut verifications)
            .await?;
        #[cfg(feature = "graph")]
        self.calibrate(&run.run_id, verifier, &claims, &verifications, &citable)?;
        for claim in &claims {
            self.store.save_claim(claim)?;
        }
        for verification in &verifications {
            self.store.save_verification(&run.run_id, verification)?;
        }
        if self.ledger {
            let so_far = self.store.list_provenance(&run.run_id)?;
            self.store.append_provenance(
                &run.run_id,
                "cost_ledger",
                &crate::ledger::totals(&so_far),
            )?;
        }
        let gaps = self.store.list_gaps(&run.run_id)?;
        let gate = gate_records(&run.run_id, &claims, &verifications, gaps.len());
        let provenance = self.store.list_provenance(&run.run_id)?;
        #[cfg_attr(not(feature = "graph"), allow(unused_mut))]
        let mut report = build_report(&run, &claims, &verifications, &gate);
        #[cfg(feature = "graph")]
        report.push_str(&crate::counter::counter_view(
            &claims,
            &verifications,
            evidence,
            &findings,
        ));
        let claim_dicts: Vec<Value> = claims.iter().map(to_value).collect();
        let verification_dicts: Vec<Value> = verifications.iter().map(to_value).collect();
        let replay = self.replay_files(&[
            ("query", Value::from(query)),
            ("draft", draft.into()),
            ("mode", mode.into()),
            ("profile", profile.into()),
            ("use_mock_empty", use_mock_empty.into()),
        ]);
        #[cfg(feature = "trust")]
        let references = references_for(&run.profile, &evidence_dicts);
        #[cfg(not(feature = "trust"))]
        let references: Option<(String, String)> = None;
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
                verifier_method: verifier.method(),
                verifier_models: &verifier.models(),
                replay: replay.as_ref(),
                signer: self.signer.as_deref(),
                references: references.as_ref().map(|(n, t)| (n.as_str(), t.as_str())),
            },
        )?;
        if self.signer.is_some() {
            let hash = crate::runpack::read_run_pack_hash(&output)?;
            self.store.append_transparency(&run.run_id, &hash)?;
        }
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

/// `references.<style>.md` in the profile's citation style. A style that
/// cannot render is reported and skipped; `sources.bib` is always there.
#[cfg(feature = "trust")]
fn references_for(profile: &str, evidence: &[Value]) -> Option<(String, String)> {
    let style = re_core::profiles::get_profile(profile).ok()?.citation_style;
    match crate::citations::render_references(evidence, style) {
        Ok(text) => Some((crate::citations::file_name(style), text)),
        Err(error) => {
            eprintln!("warning: no styled reference list: {error}");
            None
        }
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
