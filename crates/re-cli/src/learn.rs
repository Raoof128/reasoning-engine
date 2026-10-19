//! The learned prior's lifecycle (`learn` feature, plan Phase 11):
//! `prior train` fits a new version from the database's session history,
//! `prior show` describes the one in use, and the MCP server loads the
//! newest.
//!
//! Versions are `<data_dir>/prior-v<N>.json`; nothing is overwritten, so an
//! older prior can be restored by deleting newer files.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use re_core::prior::Prior;
use re_mcp::Tools;
use re_store::Store;

use crate::Failure;
use crate::config::Config;

/// `(version, path)` of every stored prior, oldest first.
fn versions(dir: &Path) -> Vec<(u32, PathBuf)> {
    let mut found: Vec<(u32, PathBuf)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let version = name.strip_prefix("prior-v")?.strip_suffix(".json")?;
            Some((version.parse().ok()?, entry.path()))
        })
        .collect();
    found.sort_by_key(|(version, _)| *version);
    found
}

/// The newest stored prior, if any; a broken file is reported and skipped.
pub fn latest(config: &Config) -> Option<(u32, Prior)> {
    let (version, path) = versions(&config.data_dir()).pop()?;
    let text = std::fs::read_to_string(&path).ok()?;
    match serde_json::from_str(&text) {
        Ok(prior) => Some((version, prior)),
        Err(error) => {
            eprintln!("warning: ignoring {}: {error}", path.display());
            None
        }
    }
}

/// The MCP tools with the newest prior, if one exists.
pub fn tools(config: &Config, tools: Tools) -> Tools {
    match latest(config) {
        Some((_, prior)) => tools.with_prior(prior),
        None => tools,
    }
}

fn describe(version: u32, prior: &Prior) -> String {
    let mut text = format!(
        "prior v{version}: {} sessions, blend weight {:.2}",
        prior.sessions,
        prior.weight()
    );
    if let Some(m) = &prior.metrics {
        let _ = write!(
            text,
            "\nheld out: {} branches, log-loss {:.4} (base rate {:.4}), accuracy {:.3}",
            m.test_examples, m.log_loss, m.base_rate_log_loss, m.accuracy
        );
    }
    text
}

/// Parse an interval: a whole number followed by `d`, `h` or `m`.
///
/// # Errors
/// Anything else, or zero.
pub fn parse_interval(text: &str) -> Result<std::time::Duration, String> {
    let text = text.trim();
    let split = text.len().saturating_sub(1);
    let (number, unit) = text.split_at(split);
    let seconds = match unit {
        "d" => 86_400,
        "h" => 3_600,
        "m" => 60,
        _ => return Err(format!("expected a number and d, h or m, like 7d: {text}")),
    };
    match number.parse::<u64>() {
        Ok(n) if n > 0 => Ok(std::time::Duration::from_secs(n * seconds)),
        _ => Err(format!(
            "expected a positive whole number before {unit}: {text}"
        )),
    }
}

/// `watch <run_id> [--every <interval>]`: once (exit 3 when anything
/// drifted), or forever at the interval.
///
/// # Errors
/// An unknown run or a pipeline failure.
pub async fn watch(
    service: &re_verify::VerifiableResearchService,
    run_id: &str,
    every: Option<std::time::Duration>,
) -> Result<u8, Failure> {
    loop {
        let report = service.watch(run_id).await?;
        crate::print_json(&report);
        let Some(interval) = every else {
            return Ok(if report["changed"] == true { 3 } else { 0 });
        };
        tokio::time::sleep(interval).await;
    }
}

/// `prior <command>`.
///
/// # Errors
/// As [`train`] and [`show`].
pub fn run(config: &Config, command: &crate::PriorCommand) -> Result<u8, Failure> {
    match command {
        crate::PriorCommand::Train => train(config),
        crate::PriorCommand::Show => show(config),
    }
}

/// `prior train`.
///
/// # Errors
/// Too little history, or store and filesystem failures.
pub fn train(config: &Config) -> Result<u8, Failure> {
    let store = Store::open(config.db_path())?;
    let examples = store.training_examples()?;
    let prior = Prior::train(&examples)?;
    let dir = config.data_dir();
    std::fs::create_dir_all(&dir)?;
    let version = versions(&dir).last().map_or(1, |(v, _)| v + 1);
    let path = dir.join(format!("prior-v{version}.json"));
    std::fs::write(&path, serde_json::to_string_pretty(&prior)? + "\n")?;
    println!("{}", describe(version, &prior));
    if let Some(m) = &prior.metrics
        && m.log_loss >= m.base_rate_log_loss
    {
        eprintln!(
            "warning: the prior does not beat the base rate on held-out sessions; \
             its blend weight keeps its influence small"
        );
    }
    println!("stored at {}", path.display());
    Ok(0)
}

/// `prior show`.
///
/// # Errors
/// None today; kept fallible like the other commands.
#[allow(clippy::unnecessary_wraps, reason = "matches the other commands")]
pub fn show(config: &Config) -> Result<u8, Failure> {
    match latest(config) {
        Some((version, prior)) => println!("{}", describe(version, &prior)),
        None => println!(
            "no prior yet: run `reasoning-engine prior train` after at least {} sessions",
            re_core::prior::MIN_SESSIONS
        ),
    }
    Ok(0)
}
