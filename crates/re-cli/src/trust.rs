//! `research --record` and `replay` (`trust` feature, plan Phase 7).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use re_retrieval::tape::Tape;
use re_retrieval::{AcademicSearchAggregator, adapters_with};
use re_store::Store;
use re_verify::replay::{RecordPlan, replay_pack, run_recorded};
use re_verify::{VerifiableResearchService, load_verifier};
use serde_json::Value;

use crate::Failure;
use crate::config::Config;

fn offline() -> bool {
    std::env::var("REASONING_ENGINE_OFFLINE").is_ok_and(|v| v == "1")
}

/// Run the pipeline with a pinned clock and seed, recording every HTTP
/// response into the run pack.
///
/// # Errors
/// As the ordinary pipeline.
pub async fn record(
    config: &Config,
    query: &str,
    draft: &str,
    mode: &str,
    profile: &str,
) -> Result<Value, Failure> {
    let secrets = [
        "REASONING_ENGINE_CONTACT_EMAIL",
        "S2_API_KEY",
        "OPENALEX_API_KEY",
        "SCHOLAR_GATEWAY_ACCESS_TOKEN",
    ]
    .into_iter()
    .filter_map(|var| std::env::var(var).ok());
    let tape = Arc::new(Tape::recording().scrubbing(secrets));
    let adapters = adapters_with(|var| std::env::var(var).ok(), Some(Arc::clone(&tape)))?;
    let sources = adapters.iter().map(|a| a.name().to_string()).collect();
    let tier = config.verifier();
    let plan = RecordPlan::now(sources, offline(), &tier);
    let verifier = load_verifier(Some(&tier), &config.models_dir(), offline()).await?;
    let service = VerifiableResearchService::new(
        Store::open(config.db_path())?,
        config.runs_dir(),
        Arc::new(AcademicSearchAggregator::new(adapters)),
        verifier,
    )
    .with_models(config.models_dir(), offline())
    .with_recorder(plan.recorder(tape));
    Ok(run_recorded(&service, &plan, query, draft, mode, profile).await?)
}

/// A run ID under the runs directory, or a run-pack directory.
fn pack_dir(config: &Config, run: &str) -> PathBuf {
    let path = Path::new(run);
    if path.is_dir() {
        path.to_path_buf()
    } else {
        config.runs_dir().join(run)
    }
}

/// Replay a recorded pack offline into a scratch directory and compare.
/// Returns the report and the exit code (0 identical, 1 otherwise).
///
/// # Errors
/// A pack that was not recorded, or filesystem failures.
pub async fn replay(config: &Config, run: &str, keep: bool) -> Result<(Value, u8), Failure> {
    let pack = pack_dir(config, run);
    if !pack.join(re_verify::runpack::REPLAY_INPUTS).exists() {
        return Err(format!(
            "{} was not recorded; run `reasoning-engine research --record ...`",
            pack.display()
        )
        .into());
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let work = std::env::temp_dir().join(format!(
        "reasoning-engine-replay-{}-{stamp}",
        std::process::id()
    ));
    let result = replay_pack(&pack, &work, &config.models_dir(), offline()).await;
    let mut report = match result {
        Ok(report) => report,
        Err(error) => {
            let _ = std::fs::remove_dir_all(&work);
            return Err(error.into());
        }
    };
    if !keep {
        let _ = std::fs::remove_dir_all(&work);
        report["replay_dir"] = Value::Null;
    }
    let code = u8::from(report["identical"] != true);
    Ok((report, code))
}
