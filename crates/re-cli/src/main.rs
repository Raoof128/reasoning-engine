//! `reasoning-engine`: stdio MCP server, research pipeline and setup.
//!
//! Subcommands and JSON output match `cli.py` (`indent=2`, sorted keys,
//! ASCII-escaped), plus `setup` and `verify-runpack`.
#![forbid(unsafe_code)]

mod config;
#[cfg(feature = "local-host")]
mod serve;
mod setup;
#[cfg(feature = "trust")]
mod trust;

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use clap::{Parser, Subcommand};
use re_core::pyjson::{DumpOptions, dumps};
use re_mcp::{ReasoningServer, Tools};
use re_retrieval::AcademicSearchAggregator;
use re_store::Store;
#[cfg(not(feature = "trust"))]
use re_verify::runpack::verify_run_pack_attestation;
use re_verify::{VerifiableResearchService, load_verifier};
use serde_json::Value;

use crate::config::{Config, config_path};

#[derive(Debug, Parser)]
#[command(
    name = "reasoning-engine",
    version,
    about = "Verifiable deep-research reasoning engine"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Configure data location, verifier and the optional local host.
    Setup(setup::SetupArgs),
    /// Serve the MCP tools over stdio (what Claude Code runs).
    Mcp,
    /// Serve the MCP tools over local Streamable HTTP (opt-in; see `setup`).
    #[cfg(feature = "local-host")]
    Serve(serve::ServeArgs),
    /// Run the verifiable research pipeline and export an attested run pack.
    Research {
        /// The research question.
        query: String,
        /// Draft answer whose claims are verified.
        #[arg(long)]
        draft: String,
        /// Research mode.
        #[arg(long, default_value = "standard")]
        mode: String,
        /// Research profile, or `auto`.
        #[arg(long, default_value = "auto")]
        profile: String,
        /// Pin the clock and ID seed and record every HTTP response into the
        /// run pack, so `replay` can re-run it offline byte for byte.
        #[cfg(feature = "trust")]
        #[arg(long)]
        record: bool,
    },
    /// Academic search.
    Scholar {
        #[command(subcommand)]
        command: ScholarCommand,
    },
    /// Measure things against labelled data.
    Bench {
        #[command(subcommand)]
        command: BenchCommand,
    },
    /// Check a run pack's attestation; exits 1 when it does not verify.
    /// With `trust`, also checks the signature and transparency log and
    /// exits 2 for an unsigned pack.
    VerifyRunpack {
        /// The run-pack directory.
        dir: PathBuf,
        /// Require this signer (base64 Ed25519 public key).
        #[cfg(feature = "trust")]
        #[arg(long)]
        public_key: Option<String>,
    },
    /// The run-pack signing key.
    #[cfg(feature = "trust")]
    Keys {
        #[command(subcommand)]
        command: KeysCommand,
    },
    /// Re-run a recorded run pack offline and compare every artifact; exits
    /// 1 when anything differs.
    #[cfg(feature = "trust")]
    Replay {
        /// A run ID (under the runs directory) or a run-pack directory.
        run: String,
        /// Keep the replayed run pack and print where it is.
        #[arg(long)]
        keep: bool,
    },
}

#[cfg(feature = "trust")]
#[derive(Debug, Subcommand)]
enum KeysCommand {
    /// Print the public key, creating the key pair if there is none.
    Show,
}

#[derive(Debug, Subcommand)]
enum BenchCommand {
    /// Precision and recall of each verifier tier on a labelled claim/evidence set.
    Verify {
        /// The labelled set (see `tests/fixtures/verify/`).
        #[arg(long, default_value = "tests/fixtures/verify/labelled_pairs.json")]
        set: PathBuf,
        /// Tiers to score; model tiers need `--features semantic`.
        #[arg(long = "tier", default_values_t = default_tiers())]
        tiers: Vec<String>,
    },
}

fn default_tiers() -> Vec<String> {
    let tiers: &[&str] = if cfg!(feature = "semantic") {
        &["lexical", "hybrid", "nli"]
    } else {
        &["lexical"]
    };
    tiers.iter().map(|t| (*t).to_string()).collect()
}

#[derive(Debug, Subcommand)]
enum ScholarCommand {
    /// Search every enabled source and record the evidence in a new run.
    Search {
        /// The query.
        query: String,
        /// Maximum results.
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
}

type Failure = Box<dyn std::error::Error + Send + Sync>;

/// The config, or defaults with a one-line hint on stderr. Never prompts.
fn load_config() -> Result<Config, Failure> {
    let path = config_path();
    Ok(Config::load(&path)?.unwrap_or_else(|| {
        eprintln!(
            "no config found at {}, run `reasoning-engine setup` (using defaults)",
            path.display()
        );
        Config::default()
    }))
}

/// The aggregator from the environment; in `tasks` builds every source
/// sits behind the evidence cache unless `REASONING_ENGINE_CACHE=off`.
#[cfg_attr(not(feature = "tasks"), allow(unused_variables))]
fn search(config: &Config, offline: bool) -> Result<AcademicSearchAggregator, Failure> {
    #[allow(unused_mut, reason = "only the tasks feature wraps the adapters")]
    let mut adapters = re_retrieval::default_adapters()?;
    #[cfg(feature = "tasks")]
    if !offline && std::env::var("REASONING_ENGINE_CACHE").as_deref() != Ok("off") {
        let cache: Arc<dyn re_retrieval::cache::CacheStore> = Arc::new(
            re_verify::ledger::StoreCache(Store::open(config.db_path())?),
        );
        adapters = re_retrieval::cache::cached(adapters, &cache);
    }
    Ok(AcademicSearchAggregator::new(adapters))
}

async fn service(config: &Config) -> Result<VerifiableResearchService, Failure> {
    let store = Store::open(config.db_path())?;
    let offline = std::env::var("REASONING_ENGINE_OFFLINE").is_ok_and(|v| v == "1");
    let models_dir = config.models_dir();
    let verifier = load_verifier(Some(&config.verifier()), &models_dir, offline).await?;
    let search = Arc::new(search(config, offline)?);
    let service = VerifiableResearchService::new(store, config.runs_dir(), search, verifier)
        .with_models(models_dir, offline);
    #[cfg(feature = "tasks")]
    let service = service.with_ledger();
    #[cfg(feature = "trust")]
    let service = trust::sign(config, service);
    Ok(service)
}

/// `print(json.dumps(payload, indent=2, sort_keys=True))`.
fn print_json(payload: &Value) {
    println!("{}", dumps(payload, &DumpOptions::indented_sorted(2)));
}

async fn run(cli: Cli) -> Result<u8, Failure> {
    match cli.command {
        Command::Setup(args) => Ok(setup::run(&args)?),
        Command::Mcp => {
            let config = load_config()?;
            let server = ReasoningServer::new(Tools::new(Arc::new(service(&config).await?)));
            server.serve_stdio().await?;
            Ok(0)
        }
        #[cfg(feature = "local-host")]
        Command::Serve(args) => {
            let config = load_config()?;
            let server = ReasoningServer::new(Tools::new(Arc::new(service(&config).await?)));
            serve::run(&config, server, &args).await
        }
        Command::Research {
            query,
            draft,
            mode,
            profile,
            #[cfg(feature = "trust")]
            record,
        } => {
            let config = load_config()?;
            #[cfg(feature = "trust")]
            if record {
                print_json(&trust::record(&config, &query, &draft, &mode, &profile).await?);
                return Ok(0);
            }
            let payload = service(&config)
                .await?
                .run_research_pipeline(&query, &draft, &mode, &profile, false)
                .await?;
            print_json(&payload);
            Ok(0)
        }
        Command::Scholar {
            command: ScholarCommand::Search { query, limit },
        } => {
            let config = load_config()?;
            let service = service(&config).await?;
            let run = service.start_run(&query, "standard", "auto")?;
            let outcome = service
                .scholar_search(&run.run_id, &query, limit, false)
                .await?;
            print_json(&outcome.to_json());
            Ok(0)
        }
        Command::Bench {
            command: BenchCommand::Verify { set, tiers },
        } => {
            let config = load_config()?;
            let text = std::fs::read_to_string(&set)
                .map_err(|e| format!("cannot read {}: {e}", set.display()))?;
            let labelled: re_verify::bench::LabelledSet = serde_json::from_str(&text)?;
            let offline = std::env::var("REASONING_ENGINE_OFFLINE").is_ok_and(|v| v == "1");
            println!(
                "Verifier benchmark on {} ({} pairs)\n",
                set.display(),
                labelled.pairs.len()
            );
            for tier in &tiers {
                let verifier = load_verifier(Some(tier), &config.models_dir(), offline).await?;
                println!(
                    "{}",
                    re_verify::bench::evaluate(verifier.as_ref(), &labelled.pairs).table()
                );
            }
            Ok(0)
        }
        #[cfg(not(feature = "trust"))]
        Command::VerifyRunpack { dir } => {
            let result = verify_run_pack_attestation(&dir);
            print_json(&result);
            Ok(u8::from(result["valid"] != true))
        }
        #[cfg(feature = "trust")]
        Command::VerifyRunpack { dir, public_key } => {
            let config = load_config()?;
            let (report, code) = trust::verify(&config, &dir, public_key.as_deref());
            print_json(&report);
            Ok(code)
        }
        #[cfg(feature = "trust")]
        Command::Keys {
            command: KeysCommand::Show,
        } => {
            print_json(&trust::show_key(&load_config()?)?);
            Ok(0)
        }
        #[cfg(feature = "trust")]
        Command::Replay { run, keep } => {
            let config = load_config()?;
            let (report, code) = trust::replay(&config, &run, keep).await?;
            print_json(&report);
            Ok(code)
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    match run(Cli::parse()).await {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}
