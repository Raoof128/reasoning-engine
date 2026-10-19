//! `reasoning-engine`: stdio MCP server, research pipeline and setup.
//!
//! Subcommands and JSON output match `cli.py` (`indent=2`, sorted keys,
//! ASCII-escaped), plus `setup` and `verify-runpack`.
#![forbid(unsafe_code)]

mod config;
mod setup;

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use clap::{Parser, Subcommand};
use re_core::pyjson::{DumpOptions, dumps};
use re_mcp::{ReasoningServer, Tools};
use re_retrieval::AcademicSearchAggregator;
use re_store::Store;
use re_verify::runpack::verify_run_pack_attestation;
use re_verify::{VerifiableResearchService, verifier_from_name};
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
    },
    /// Academic search.
    Scholar {
        #[command(subcommand)]
        command: ScholarCommand,
    },
    /// Check a run pack's attestation; exits 1 when it does not verify.
    VerifyRunpack {
        /// The run-pack directory.
        dir: PathBuf,
    },
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

fn service(config: &Config) -> Result<VerifiableResearchService, Failure> {
    let store = Store::open(config.db_path())?;
    let verifier = verifier_from_name(Some(&config.verifier()))?;
    let search = Arc::new(AcademicSearchAggregator::from_env()?);
    Ok(VerifiableResearchService::new(
        store,
        config.runs_dir(),
        search,
        verifier,
    ))
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
            let server = ReasoningServer::new(Tools::new(Arc::new(service(&config)?)));
            server.serve_stdio().await?;
            Ok(0)
        }
        Command::Research {
            query,
            draft,
            mode,
            profile,
        } => {
            let config = load_config()?;
            let payload = service(&config)?
                .run_research_pipeline(&query, &draft, &mode, &profile, false)
                .await?;
            print_json(&payload);
            Ok(0)
        }
        Command::Scholar {
            command: ScholarCommand::Search { query, limit },
        } => {
            let config = load_config()?;
            let service = service(&config)?;
            let run = service.start_run(&query, "standard", "auto")?;
            let outcome = service
                .scholar_search(&run.run_id, &query, limit, false)
                .await?;
            print_json(&outcome.to_json());
            Ok(0)
        }
        Command::VerifyRunpack { dir } => {
            let result = verify_run_pack_attestation(&dir);
            print_json(&result);
            Ok(u8::from(result["valid"] != true))
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
