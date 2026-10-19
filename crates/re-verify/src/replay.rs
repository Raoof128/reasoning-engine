//! Recorded runs and their offline replay (plan Phase 7).
//!
//! `research --record` pins the clock and identifier seed
//! ([`re_model::pin`]), records every HTTP response on a
//! [`re_retrieval::tape::Tape`], and exports `replay.json` plus `http/`
//! beside the usual artifacts. [`replay_pack`] re-runs the same pipeline
//! from those files with no network, into a scratch database and runs
//! directory, and compares every artifact hash with the original
//! attestation. A replay that matches is byte-identical; one that does not
//! names the artifacts that changed.

use std::path::Path;
use std::sync::Arc;

use re_model::clock::utc_now;
use re_model::pin::pin;
use re_retrieval::tape::Tape;
use re_retrieval::{AcademicSearchAggregator, adapters_with};
use re_store::Store;
use serde_json::{Map, Value, json};

use crate::error::VerifyError;
use crate::runpack::{ARTIFACTS, RECORDINGS_DIR, REPLAY_INPUTS, verify_run_pack_attestation};
use crate::service::{Recorder, VerifiableResearchService};
use crate::verify::load_verifier;

/// A fresh seed from the operating system's randomness.
#[must_use]
pub fn new_seed() -> u64 {
    // 16 hex digits of a v4 UUID's random part always parse.
    u64::from_str_radix(&re_model::ids::short_hex(16), 16).unwrap_or_default()
}

/// How a run is recorded: what `replay.json` needs besides the pipeline's
/// own inputs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordPlan {
    /// The pinned clock (`YYYY-MM-DDTHH:MM:SSZ`).
    pub clock: String,
    /// The identifier seed.
    pub seed: u64,
    /// Enabled source names, in aggregator order.
    pub sources: Vec<String>,
    /// Whether the run used the offline mock.
    pub offline: bool,
    /// The configured verifier tier.
    pub verifier: String,
}

impl RecordPlan {
    /// A plan for a run starting now with a fresh seed.
    #[must_use]
    pub fn now(sources: Vec<String>, offline: bool, verifier: &str) -> Self {
        Self {
            clock: utc_now(),
            seed: new_seed(),
            sources,
            offline,
            verifier: verifier.to_string(),
        }
    }

    /// The recorder to give the service, recording on `tape`.
    #[must_use]
    pub fn recorder(&self, tape: Arc<Tape>) -> Recorder {
        let mut settings = Map::new();
        settings.insert("format".into(), 1.into());
        settings.insert("engine_version".into(), env!("CARGO_PKG_VERSION").into());
        settings.insert("clock".into(), self.clock.clone().into());
        // A string: JSON readers that use doubles would round a u64.
        settings.insert("seed".into(), format!("{:016x}", self.seed).into());
        settings.insert("sources".into(), self.sources.clone().into());
        settings.insert("offline".into(), self.offline.into());
        settings.insert("verifier".into(), self.verifier.clone().into());
        Recorder { tape, settings }
    }
}

fn invalid(message: impl Into<String>) -> VerifyError {
    VerifyError::Io(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        message.into(),
    ))
}

fn read_json(path: &Path) -> Result<Value, VerifyError> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| invalid(format!("cannot read {}: {e}", path.display())))?;
    serde_json::from_str(&text).map_err(|e| invalid(format!("invalid {}: {e}", path.display())))
}

fn text<'a>(inputs: &'a Value, key: &str) -> Result<&'a str, VerifyError> {
    inputs
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| invalid(format!("{REPLAY_INPUTS} has no {key}")))
}

/// Re-run the recorded pack at `pack_dir` offline, writing into
/// `work_dir`, and compare artifacts with its attestation.
///
/// Model tiers load from `models_dir`; `offline_models` forbids
/// downloading them.
///
/// # Errors
/// A pack without `replay.json` or `attestation.json`, an unknown
/// verifier, or store and filesystem failures. Differences in artifacts are
/// reported in the result, not as errors.
pub async fn replay_pack(
    pack_dir: &Path,
    work_dir: &Path,
    models_dir: &Path,
    offline_models: bool,
) -> Result<Value, VerifyError> {
    let inputs = read_json(&pack_dir.join(REPLAY_INPUTS))?;
    let attestation = read_json(&pack_dir.join("attestation.json"))?;
    let expected = attestation
        .get("artifact_hashes")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let seed = u64::from_str_radix(text(&inputs, "seed")?, 16)
        .map_err(|e| invalid(format!("bad seed in {REPLAY_INPUTS}: {e}")))?;
    let clock = text(&inputs, "clock")?.to_string();
    let sources: Vec<String> = inputs
        .get("sources")
        .and_then(Value::as_array)
        .map(|names| {
            names
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let offline = inputs.get("offline").and_then(Value::as_bool) == Some(true);

    let tape = Arc::new(Tape::replaying(&pack_dir.join(RECORDINGS_DIR))?);
    let joined = sources.join(",");
    let adapters = adapters_with(
        |var| match var {
            "REASONING_ENGINE_OFFLINE" if offline => Some("1".into()),
            "REASONING_ENGINE_SOURCES" => Some(joined.clone()),
            _ => None,
        },
        Some(tape),
    )?;
    let verifier =
        load_verifier(Some(text(&inputs, "verifier")?), models_dir, offline_models).await?;
    std::fs::create_dir_all(work_dir)?;
    let store = Store::open(work_dir.join("replay.db"))?;
    let service = VerifiableResearchService::new(
        store,
        work_dir.join("runs"),
        Arc::new(AcademicSearchAggregator::new(adapters)),
        verifier,
    )
    .with_models(models_dir, offline_models);

    let result = {
        let _pinned = pin(&clock, seed);
        service
            .run_research_pipeline(
                text(&inputs, "query")?,
                text(&inputs, "draft")?,
                text(&inputs, "mode")?,
                text(&inputs, "profile")?,
                inputs.get("use_mock_empty").and_then(Value::as_bool) == Some(true),
            )
            .await?
    };
    let replay_dir = result
        .get("run_pack")
        .and_then(Value::as_str)
        .map(std::path::PathBuf::from)
        .ok_or_else(|| invalid("the replayed pipeline exported no run pack"))?;

    let mut mismatched = Vec::new();
    let mut missing = Vec::new();
    for name in ARTIFACTS {
        let Some(want) = expected.get(name).and_then(Value::as_str) else {
            missing.push(name);
            continue;
        };
        let got = std::fs::read(replay_dir.join(name))
            .map(|bytes| re_core::hashing::sha256_prefixed(&bytes))
            .ok();
        if got.as_deref() != Some(want) {
            mismatched.push(name);
        }
    }
    let original_run = attestation.get("run_id").cloned().unwrap_or(Value::Null);
    Ok(json!({
        "identical": mismatched.is_empty() && missing.is_empty(),
        "run_id": original_run,
        "replayed_run_id": result.get("run_id").cloned().unwrap_or(Value::Null),
        "mismatched_artifacts": mismatched,
        "missing_artifacts": missing,
        "replay_dir": replay_dir.display().to_string(),
        "pack_attestation": verify_run_pack_attestation(pack_dir),
    }))
}

/// Run the pipeline under `plan`'s pinned clock and seed. `service` must
/// have been built with `plan.recorder(tape)` and an aggregator recording
/// on the same tape.
///
/// # Errors
/// As [`VerifiableResearchService::run_research_pipeline`].
pub async fn run_recorded(
    service: &VerifiableResearchService,
    plan: &RecordPlan,
    query: &str,
    draft: &str,
    mode: &str,
    profile: &str,
) -> Result<Value, VerifyError> {
    let _pinned = pin(&plan.clock, plan.seed);
    service
        .run_research_pipeline(query, draft, mode, profile, false)
        .await
}
