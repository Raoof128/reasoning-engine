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
    /// Fit the NLI tier's conformal calibration on labelled pairs, report
    /// held-out coverage, and store it for research runs.
    #[cfg(feature = "graph")]
    Calibrate {
        /// The labelled set (see `tests/fixtures/verify/`).
        #[arg(long, default_value = "tests/fixtures/verify/labelled_pairs.json")]
        set: PathBuf,
        /// Target miscoverage: 0.1 aims for 90% coverage.
        #[arg(long, default_value_t = re_verify::calibration::DEFAULT_ALPHA)]
        alpha: f64,
    },
    /// Precision and recall of each verifier tier on a labelled claim/evidence set.
    Verify {
        /// The labelled set (see `tests/fixtures/verify/`).
        #[arg(long, default_value = "tests/fixtures/verify/labelled_pairs.json")]
        set: PathBuf,
        /// Tiers to score; model tiers need `--features semantic`.
        #[arg(long = "tier", default_values_t = default_tiers())]
        tiers: Vec<String>,
        /// Instead, compare abstract-only with full-text verification on
        /// this set of claims about open-access papers
        /// (`tests/fixtures/verify/fulltext_pairs.json`).
        #[cfg(feature = "fulltext")]
        #[arg(long = "full-text", value_name = "SET")]
        full_text: Option<PathBuf>,
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
    #[cfg(feature = "fulltext")]
    let service = {
        let service = service.with_quality();
        if offline {
            service
        } else {
            let fetcher =
                re_retrieval::fulltext::FullTextFetcher::new(re_retrieval::HttpClient::new()?)
                    .with_cache_dir(config.fulltext_dir());
            service.with_full_text(fetcher)
        }
    };
    #[cfg(feature = "graph")]
    let service = match load_calibration(config) {
        Some(calibration) => service.with_counter_search().with_calibration(calibration),
        None => service.with_counter_search(),
    };
    #[cfg(feature = "graph")]
    let service = if offline {
        service
    } else {
        service.with_evidence_graph(re_retrieval::links::LinkFetcher::new(
            re_retrieval::HttpClient::new()?,
            &re_retrieval::sources::SourceConfig::from_env(),
        ))
    };
    #[cfg(feature = "trust")]
    let service = trust::sign(config, service);
    Ok(service)
}

/// The stored calibration, if any; a broken file is reported and ignored.
#[cfg(feature = "graph")]
fn load_calibration(config: &Config) -> Option<re_verify::calibration::Calibration> {
    let path = config.calibration_path();
    if !path.exists() {
        return None;
    }
    re_verify::calibration::Calibration::load(&path)
        .inspect_err(|e| eprintln!("warning: ignoring calibration: {e}"))
        .ok()
}

/// `bench calibrate`: score every labelled pair with the NLI tier, fit on
/// a deterministic half, report coverage on the other half, then fit on
/// all pairs (plus `<data_dir>/labelled_pairs.json`, if present) and store
/// the result.
#[cfg(feature = "graph")]
async fn bench_calibrate(set: &std::path::Path, alpha: f64) -> Result<u8, Failure> {
    use re_verify::bench::{LABELS, LabelledSet, Pair};
    use re_verify::calibration::{Calibration, split};
    if !(0.0..1.0).contains(&alpha) || alpha == 0.0 {
        return Err(format!("--alpha must be between 0 and 1, got {alpha}").into());
    }
    let config = load_config()?;
    let read = |path: &std::path::Path| -> Result<Vec<Pair>, Failure> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        Ok(serde_json::from_str::<LabelledSet>(&text)?.pairs)
    };
    let mut pairs = read(set)?;
    let user = config.data_dir().join("labelled_pairs.json");
    if user.exists() {
        pairs.extend(read(&user)?);
    }
    let offline = std::env::var("REASONING_ENGINE_OFFLINE").is_ok_and(|v| v == "1");
    let verifier = load_verifier(Some("nli"), &config.models_dir(), offline).await?;
    let model = verifier.models().join(", ");
    let mut probabilities = Vec::with_capacity(pairs.len());
    let mut gold = Vec::with_capacity(pairs.len());
    let mut ids = Vec::with_capacity(pairs.len());
    for pair in &pairs {
        let Some(p) = verifier.probabilities(&pair.evidence, &pair.claim) else {
            return Err("the nli tier gave no probabilities".into());
        };
        probabilities.push(p);
        gold.push(LABELS.iter().position(|l| *l == pair.label).unwrap_or(2));
        ids.push(pair.id.as_str());
    }
    let (fit_on, held_out) = split(&ids);
    let pick = |indices: &[usize]| -> (Vec<[f64; 3]>, Vec<usize>) {
        indices.iter().map(|&i| (probabilities[i], gold[i])).unzip()
    };
    let (fit_p, fit_g) = pick(&fit_on);
    let (test_p, test_g) = pick(&held_out);
    let half = Calibration::fit(&fit_p, &fit_g, alpha, &model)
        .ok_or_else(|| "no pairs to calibrate on".to_string())?;
    let sets: usize = test_p.iter().map(|p| half.prediction_set(p).len()).sum();
    #[allow(clippy::cast_precision_loss, reason = "counts of labelled pairs")]
    let mean_set = sets as f64 / test_p.len().max(1) as f64;
    println!(
        "Conformal calibration of {model} on {} pairs (alpha {alpha})\n\n\
         | Split | Pairs | Quantile | Coverage | Mean set size |\n|---|---:|---:|---:|---:|\n\
         | fit {} / held out {} | {} | {:.3} | {:.3} (target {:.3}) | {mean_set:.2} |",
        pairs.len(),
        fit_on.len(),
        held_out.len(),
        test_p.len(),
        half.quantile,
        half.coverage(&test_p, &test_g),
        1.0 - alpha,
    );
    let full = Calibration::fit(&probabilities, &gold, alpha, &model)
        .ok_or_else(|| "no pairs to calibrate on".to_string())?;
    let path = config.calibration_path();
    full.save(&path)?;
    println!(
        "\nStored the calibration on all {} pairs (quantile {:.3}) at {}",
        full.n,
        full.quantile,
        path.display()
    );
    Ok(0)
}

/// `bench verify`: each tier on the labelled claim/sentence set.
async fn bench_verify(set: &std::path::Path, tiers: &[String]) -> Result<u8, Failure> {
    let config = load_config()?;
    let text =
        std::fs::read_to_string(set).map_err(|e| format!("cannot read {}: {e}", set.display()))?;
    let labelled: re_verify::bench::LabelledSet = serde_json::from_str(&text)?;
    let offline = std::env::var("REASONING_ENGINE_OFFLINE").is_ok_and(|v| v == "1");
    println!(
        "Verifier benchmark on {} ({} pairs)\n",
        set.display(),
        labelled.pairs.len()
    );
    for tier in tiers {
        let verifier = load_verifier(Some(tier), &config.models_dir(), offline).await?;
        println!(
            "{}",
            re_verify::bench::evaluate(verifier.as_ref(), &labelled.pairs).table()
        );
    }
    Ok(0)
}

/// `bench verify --full-text`: abstract-only against full text, per tier.
#[cfg(feature = "fulltext")]
async fn bench_full_text(set: &std::path::Path, tiers: &[String]) -> Result<u8, Failure> {
    let config = load_config()?;
    let text =
        std::fs::read_to_string(set).map_err(|e| format!("cannot read {}: {e}", set.display()))?;
    let labelled: re_verify::bench::FullTextSet = serde_json::from_str(&text)?;
    let dir = set.parent().unwrap_or_else(|| std::path::Path::new("."));
    let offline = std::env::var("REASONING_ENGINE_OFFLINE").is_ok_and(|v| v == "1");
    println!(
        "Full-text verifier benchmark on {} ({} claims, {} papers)\n",
        set.display(),
        labelled.pairs.len(),
        labelled.documents.len()
    );
    for tier in tiers {
        let verifier = load_verifier(Some(tier), &config.models_dir(), offline).await?;
        let [abstract_only, chunked, full] =
            re_verify::bench::evaluate_full_text(verifier.as_ref(), &labelled, dir)?;
        println!(
            "{}\n{}\n{}",
            abstract_only.table(),
            chunked.table(),
            full.table()
        );
        println!(
            "partially_supported precision: {:.3} abstract only, {:.3} abstract in chunks, \
             {:.3} full text\n",
            abstract_only.precision("partially_supported"),
            chunked.precision("partially_supported"),
            full.precision("partially_supported")
        );
    }
    Ok(0)
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
        #[cfg(feature = "graph")]
        Command::Bench {
            command: BenchCommand::Calibrate { set, alpha },
        } => bench_calibrate(&set, alpha).await,
        #[cfg(feature = "fulltext")]
        Command::Bench {
            command:
                BenchCommand::Verify {
                    tiers,
                    full_text: Some(set),
                    ..
                },
        } => bench_full_text(&set, &tiers).await,
        Command::Bench {
            command: BenchCommand::Verify { set, tiers, .. },
        } => bench_verify(&set, &tiers).await,
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
