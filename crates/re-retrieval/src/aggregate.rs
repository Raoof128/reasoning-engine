//! Concurrent fan-out over every enabled source, with dedup and
//! deterministic ranking (port of `verifiable/aggregator.py`).

use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};

use indexmap::IndexMap;
use re_model::records::{EvidenceRecord, RetrievalError};
use regex::Regex;
use serde_json::{Map, Value};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use crate::base::{DEFAULT_CONTACT_EMAIL, py_strip};
use crate::client::HttpClient;
use crate::error::AdapterError;
use crate::result::RetrievalResult;
use crate::sources::mock::MockScholarGateway;
use crate::sources::{FREE_SOURCES, HttpAdapter, SourceAdapter, SourceConfig, free_source};
use crate::tape::Tape;

/// Python's `ThreadPoolExecutor(max_workers=...)` cap on in-flight searches.
pub const MAX_WORKERS: usize = 8;

/// Everything Python's `str.isalnum` rejects: not a letter (`L*`) and not a
/// number (`N*`). Rust's `char::is_alphanumeric` also keeps combining marks
/// (`Other_Alphabetic`), which would split dedup keys from Python's.
static NOT_ALNUM: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[^\p{L}\p{N}]+").expect("valid regex"));

/// A shared, sendable adapter.
pub type Adapter = Arc<dyn SourceAdapter>;

fn normalized_title(title: &str) -> String {
    NOT_ALNUM
        .replace_all(&title.to_lowercase(), "")
        .into_owned()
}

/// `doi:<doi>` when there is a DOI, else `title:<letters and digits>`.
#[must_use]
pub fn dedup_key(record: &EvidenceRecord) -> String {
    match record.doi.as_deref() {
        Some(doi) if !doi.is_empty() => format!("doi:{doi}"),
        _ => format!("title:{}", normalized_title(&record.title)),
    }
}

/// Build the enabled adapters from the environment: the mock alone when
/// `REASONING_ENGINE_OFFLINE=1`, else `REASONING_ENGINE_SOURCES` (comma
/// separated, unknown names ignored) or every free source. Scholar Gateway
/// joins only when compiled in and configured; with `wasm-adapters`, other
/// requested names load from the adapters directory.
///
/// # Errors
/// If the shared HTTP client cannot be built.
pub fn default_adapters() -> Result<Vec<Adapter>, AdapterError> {
    adapters_from(|var| std::env::var(var).ok())
}

/// [`default_adapters`] with an explicit environment, for tests and for
/// callers that hold configuration elsewhere.
///
/// # Errors
/// If the shared HTTP client cannot be built.
pub fn adapters_from(env: impl Fn(&str) -> Option<String>) -> Result<Vec<Adapter>, AdapterError> {
    adapters_with(env, None)
}

/// [`adapters_from`] with the shared HTTP client recording to, or replaying
/// from, `tape`. Scholar Gateway (MCP, token-gated) is never taped.
///
/// # Errors
/// If the shared HTTP client cannot be built.
pub fn adapters_with(
    env: impl Fn(&str) -> Option<String>,
    tape: Option<Arc<Tape>>,
) -> Result<Vec<Adapter>, AdapterError> {
    if env("REASONING_ENGINE_OFFLINE").as_deref() == Some("1") {
        return Ok(vec![Arc::new(MockScholarGateway)]);
    }
    let requested = env("REASONING_ENGINE_SOURCES").unwrap_or_default();
    let requested = py_strip(&requested);
    let names: Vec<&str> = if requested.is_empty() {
        FREE_SOURCES.to_vec()
    } else {
        requested
            .split(',')
            .map(py_strip)
            .filter(|n| !n.is_empty())
            .collect()
    };
    // One pooled client: every source reuses its connections and TLS state.
    let client = match tape {
        Some(tape) => HttpClient::new()?.with_tape(tape),
        None => HttpClient::new()?,
    };
    let non_empty = |var: &str| env(var).filter(|v| !v.is_empty());
    let config = SourceConfig {
        contact_email: env("REASONING_ENGINE_CONTACT_EMAIL")
            .unwrap_or_else(|| DEFAULT_CONTACT_EMAIL.into()),
        semantic_scholar_api_key: non_empty("S2_API_KEY"),
        openalex_api_key: non_empty("OPENALEX_API_KEY"),
    };
    #[allow(unused_mut, reason = "only the scholar-gateway feature appends")]
    let mut adapters: Vec<Adapter> = names
        .iter()
        .filter_map(|name| free_source(name, &config))
        .map(|source| Arc::new(HttpAdapter::new(source, client.clone())) as Adapter)
        .collect();
    #[cfg(feature = "scholar-gateway")]
    {
        use crate::sources::scholar_gateway::ScholarGateway;
        let configured = env("SCHOLAR_GATEWAY_LIVE").as_deref() == Some("1")
            && non_empty("SCHOLAR_GATEWAY_ACCESS_TOKEN").is_some();
        if configured && (requested.is_empty() || names.contains(&"scholar_gateway")) {
            adapters.push(Arc::new(ScholarGateway::new()?));
        }
    }
    #[cfg(feature = "wasm-adapters")]
    {
        // Names that are not built in may be sandboxed adapters on disk.
        let others: Vec<&str> = names
            .iter()
            .copied()
            .filter(|name| *name != "scholar_gateway" && free_source(name, &config).is_none())
            .collect();
        if let Some(dir) = crate::wasm::adapters_dir(&env)
            && !others.is_empty()
        {
            adapters.extend(
                crate::wasm::load(&dir, &others, &client)
                    .into_iter()
                    .map(|adapter| Arc::new(adapter) as Adapter),
            );
        }
    }
    Ok(adapters)
}

fn retrieval_error(
    error_type: &str,
    message: impl Into<String>,
    retryable: bool,
) -> RetrievalError {
    RetrievalError {
        error_type: error_type.into(),
        message: message.into(),
        retryable,
        metadata: Map::new(),
    }
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "panic".into())
}

/// How long a fan-out waits for slow sources.
///
/// One slow provider used to set the pace for every search: Europe PMC
/// routinely takes 10 to 16 seconds while the others answer in under two,
/// and a search returned only when the last one did (or timed out at 25).
/// With a policy, the fan-out returns once enough sources have answered and
/// the stragglers have had a short grace period.
///
/// A run that is being recorded or replayed never gets a policy: cutting a
/// source off depends on the clock, and a replay must reproduce the
/// recording byte for byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FanoutPolicy {
    /// Sources that must have returned evidence before the grace period
    /// starts. Capped at the number of adapters. The grace period also
    /// starts when only one source is still out and another has evidence.
    pub quorum: usize,
    /// Once the quorum has answered, how long the rest get.
    pub grace: Duration,
    /// Past this, return what has arrived, as long as something has. With
    /// nothing in hand the fan-out keeps waiting: an empty answer is worse
    /// than a late one.
    pub deadline: Duration,
}

impl FanoutPolicy {
    /// Three sources, then 1.5 s for the rest, and never more than 8 s.
    pub const DEFAULT: Self = Self {
        quorum: 3,
        grace: Duration::from_millis(1500),
        deadline: Duration::from_secs(8),
    };

    /// The policy from `REASONING_ENGINE_FANOUT_DEADLINE_SECS`: unset means
    /// [`Self::DEFAULT`], `0` means wait for every source (the behaviour
    /// before the policy existed), and any other whole number of seconds
    /// replaces the deadline.
    pub fn from_env(env: impl Fn(&str) -> Option<String>) -> Option<Self> {
        match env("REASONING_ENGINE_FANOUT_DEADLINE_SECS")
            .as_deref()
            .map(str::trim)
        {
            None | Some("") => Some(Self::DEFAULT),
            Some(raw) => match raw.parse::<u64>() {
                Ok(0) => None,
                Ok(secs) => Some(Self {
                    deadline: Duration::from_secs(secs),
                    ..Self::DEFAULT
                }),
                Err(_) => Some(Self::DEFAULT),
            },
        }
    }
}

/// Searches every adapter at once and merges the results.
pub struct AcademicSearchAggregator {
    adapters: Vec<Adapter>,
    fanout: Option<FanoutPolicy>,
}

impl AcademicSearchAggregator {
    /// Python's `name` attribute.
    pub const NAME: &'static str = "academic_aggregator";

    /// Aggregate over `adapters`, in that order.
    #[must_use]
    pub fn new(adapters: Vec<Adapter>) -> Self {
        Self {
            adapters,
            fanout: None,
        }
    }

    /// Stop waiting for slow sources as `policy` says. `None` waits for
    /// every source, which is what [`Self::new`] does.
    #[must_use]
    pub fn with_fanout(mut self, policy: Option<FanoutPolicy>) -> Self {
        self.fanout = policy;
        self
    }

    /// Aggregate over [`default_adapters`].
    ///
    /// # Errors
    /// If the shared HTTP client cannot be built.
    pub fn from_env() -> Result<Self, AdapterError> {
        let policy = FanoutPolicy::from_env(|var| std::env::var(var).ok());
        default_adapters().map(|adapters| Self::new(adapters).with_fanout(policy))
    }

    /// The adapters, in fan-out order.
    #[must_use]
    pub fn adapters(&self) -> &[Adapter] {
        &self.adapters
    }

    /// Search every source, dedup, rank and keep the best `limit`.
    ///
    /// Per-source failures surface in `error` only when no source returned
    /// evidence, as in Python. Rust-only `diagnostics` are keyed by source.
    pub async fn search(&self, run_id: &str, query: &str, limit: usize) -> RetrievalResult {
        let query = py_strip(query);
        if query.is_empty() {
            return RetrievalResult::failed(retrieval_error(
                "unsupported_query",
                "query must not be empty",
                false,
            ));
        }
        if self.adapters.is_empty() {
            return RetrievalResult::failed(retrieval_error(
                "adapter_misconfigured",
                "no search adapters are enabled",
                false,
            ));
        }

        let results = self.run_adapters(run_id, query, limit).await;

        let mut collected = Vec::new();
        let mut errors: IndexMap<&'static str, RetrievalError> = IndexMap::new();
        let mut diagnostics = Map::new();
        for (name, result) in results {
            if let Some(error) = result.error {
                errors.insert(name, error);
            }
            if !result.diagnostics.is_empty() {
                diagnostics.insert(name.into(), Value::Object(result.diagnostics));
            }
            collected.extend(result.evidence);
        }

        if collected.is_empty() {
            return RetrievalResult {
                evidence: Vec::new(),
                error: aggregate_error(&errors),
                diagnostics,
            };
        }

        let mut merged: Vec<(String, EvidenceRecord)> =
            merge_same_work(dedup(collected)).into_iter().collect();
        // Stable sort on (-score, source_adapter, dedup_key), like Python's.
        merged.sort_by(|(ka, a), (kb, b)| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(Ordering::Equal)
                .then_with(|| a.source_adapter.cmp(&b.source_adapter))
                .then_with(|| ka.cmp(kb))
        });
        let evidence = merged
            .into_iter()
            .take(limit)
            .zip(1i64..)
            .map(|((_, mut record), index)| {
                record.evidence_id = format!("ev_{index:04}");
                record.rank = index;
                record
            })
            .collect();
        RetrievalResult {
            evidence,
            error: None,
            diagnostics,
        }
    }

    /// Run every adapter concurrently (at most [`MAX_WORKERS`] at a time),
    /// returning results in adapter order. A panicking adapter becomes an
    /// `unavailable` error for that source only. Under a [`FanoutPolicy`],
    /// a source still running when the policy says stop is left to finish
    /// in the background and reported as `deadline_exceeded`, with how long
    /// it was given in its diagnostics.
    async fn run_adapters(
        &self,
        run_id: &str,
        query: &str,
        limit: usize,
    ) -> Vec<(&'static str, RetrievalResult)> {
        let permits = Arc::new(Semaphore::new(MAX_WORKERS.min(self.adapters.len())));
        let mut tasks = JoinSet::new();
        // Task id -> adapter index, so a panicked task still maps to its slot.
        let mut slot_of = HashMap::new();
        for (index, adapter) in self.adapters.iter().enumerate() {
            let (adapter, permits) = (Arc::clone(adapter), Arc::clone(&permits));
            let (run_id, query) = (run_id.to_string(), query.to_string());
            let handle = tasks.spawn(async move {
                // The semaphore is never closed, so acquire cannot fail.
                let _permit = permits.acquire_owned().await;
                adapter.search(&run_id, &query, limit).await
            });
            slot_of.insert(handle.id(), index);
        }
        let mut slots: Vec<Option<RetrievalResult>> = vec![None; self.adapters.len()];
        let started = Instant::now();
        let quorum = self
            .fanout
            .map_or(usize::MAX, |p| p.quorum.clamp(1, self.adapters.len()));
        let mut with_evidence = 0_usize;
        let mut answered = 0_usize;
        // When the grace period for stragglers ends, once it has started.
        let mut grace_ends: Option<Instant> = None;
        loop {
            // The moment to stop waiting, if the policy has one yet: the end
            // of the grace period, or the deadline once anything is in hand.
            let stop_at = self.fanout.and_then(|policy| {
                let deadline = (with_evidence > 0).then(|| started + policy.deadline);
                match (grace_ends, deadline) {
                    (Some(a), Some(b)) => Some(a.min(b)),
                    (a, b) => a.or(b),
                }
            });
            let next = match stop_at {
                Some(at) => {
                    match tokio::time::timeout_at(at.into(), tasks.join_next_with_id()).await {
                        Ok(next) => next,
                        Err(_) => break,
                    }
                }
                None => tasks.join_next_with_id().await,
            };
            let Some(joined) = next else { break };
            let (id, result) = match joined {
                Ok((id, result)) => (id, result),
                Err(error) => {
                    let id = error.id();
                    let detail = if error.is_panic() {
                        panic_message(&*error.into_panic())
                    } else {
                        "cancelled".into()
                    };
                    let name = slot_of
                        .get(&id)
                        .map_or("unknown", |&i| self.adapters[i].name());
                    let message = format!("{name} adapter crashed: {detail}");
                    (
                        id,
                        RetrievalResult::failed(retrieval_error("unavailable", message, true)),
                    )
                }
            };
            answered += 1;
            if !result.evidence.is_empty() {
                with_evidence += 1;
            }
            // The grace period starts when the quorum has evidence, or when
            // a single source is all that is left and something is in hand:
            // sources that fail or come back empty must not make the search
            // wait out the full deadline for the last one.
            let one_left = answered + 1 >= self.adapters.len() && with_evidence > 0;
            if (with_evidence >= quorum || one_left) && grace_ends.is_none() {
                grace_ends = self.fanout.map(|policy| Instant::now() + policy.grace);
            }
            if let Some(&index) = slot_of.get(&id) {
                slots[index] = Some(result);
            }
        }
        // Whatever is still running was left behind. It is detached, not
        // cancelled: a cached source that finishes late still stores its
        // answer, so the same query asked again includes it at once.
        let waited_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        tasks.detach_all();
        self.adapters
            .iter()
            .zip(slots)
            .map(|(adapter, slot)| {
                let result = slot.unwrap_or_else(|| {
                    let mut cut = RetrievalResult::failed(retrieval_error(
                        "deadline_exceeded",
                        format!(
                            "{} had not answered after {waited_ms} ms; the search returned \
                             without it",
                            adapter.name()
                        ),
                        true,
                    ));
                    cut.diagnostics
                        .insert("deadline_exceeded".into(), Value::Bool(true));
                    cut.diagnostics
                        .insert("waited_ms".into(), Value::from(waited_ms));
                    cut
                });
                (adapter.name(), result)
            })
            .collect()
    }
}

fn also_found_in(metadata: &Map<String, Value>) -> Vec<Value> {
    metadata
        .get("also_found_in")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

/// Keep the best-scoring record per dedup key, recording where else it was
/// found. First-seen key order is preserved, as with a Python dict.
fn dedup(records: Vec<EvidenceRecord>) -> IndexMap<String, EvidenceRecord> {
    let mut best: IndexMap<String, EvidenceRecord> = IndexMap::new();
    for mut record in records {
        let key = dedup_key(&record);
        match best.get_mut(&key) {
            Some(existing) if record.score > existing.score => {
                let mut also = also_found_in(&existing.metadata);
                also.push(Value::String(existing.source_adapter.clone()));
                record
                    .metadata
                    .insert("also_found_in".into(), Value::Array(also));
                *existing = record;
            }
            Some(existing) => {
                let mut also = also_found_in(&existing.metadata);
                also.push(Value::String(record.source_adapter));
                existing
                    .metadata
                    .insert("also_found_in".into(), Value::Array(also));
            }
            None => {
                best.insert(key, record);
            }
        }
    }
    best
}

/// A normalised title shorter than this is too generic to identify a work
/// ("Introduction", "Editorial").
const MIN_TITLE_KEY: usize = 24;

fn years_agree(a: Option<i64>, b: Option<i64>) -> bool {
    match (a, b) {
        // A preprint and its published version are often a year or two apart.
        (Some(a), Some(b)) => (a - b).abs() <= 2,
        _ => true,
    }
}

/// Merge records that are one work under different identifiers.
///
/// [`dedup`] keys on the DOI, and a paper usually has several: the
/// publisher's, the arXiv one, and none at all in a source that does not
/// carry DOIs. One paper then took three of a search's ten places. Records
/// whose titles are the same letters and digits, and whose years are within
/// two of each other, are one work: the best-scoring record is kept, takes
/// an abstract or DOI from the others if it lacks one, and lists where else
/// it was found.
fn merge_same_work(records: IndexMap<String, EvidenceRecord>) -> IndexMap<String, EvidenceRecord> {
    let mut out: IndexMap<String, EvidenceRecord> = IndexMap::new();
    // Normalised title -> the key in `out` that holds the work.
    let mut by_title: HashMap<String, String> = HashMap::new();
    for (key, mut record) in records {
        let title = normalized_title(&record.title);
        if title.chars().count() < MIN_TITLE_KEY {
            out.insert(key, record);
            continue;
        }
        let held = by_title
            .get(&title)
            .filter(|held| {
                out.get(*held)
                    .is_some_and(|e| years_agree(e.year, record.year))
            })
            .cloned();
        let Some(held) = held else {
            by_title.entry(title).or_insert_with(|| key.clone());
            out.insert(key, record);
            continue;
        };
        let Some(existing) = out.get_mut(&held) else {
            continue;
        };
        if record.score > existing.score {
            std::mem::swap(existing, &mut record);
        }
        // `existing` is the keeper; `record` is the one folded into it.
        let mut also = also_found_in(&existing.metadata);
        also.push(Value::String(record.source_adapter.clone()));
        also.extend(also_found_in(&record.metadata));
        also.dedup();
        existing
            .metadata
            .insert("also_found_in".into(), Value::Array(also));
        if existing.snippet.trim().is_empty() && !record.snippet.trim().is_empty() {
            existing.snippet = record.snippet;
        }
        if existing.doi.as_deref().is_none_or(str::is_empty) {
            existing.doi = record.doi;
        }
        if existing.url.as_deref().is_none_or(str::is_empty) {
            existing.url = record.url;
        }
    }
    out
}

/// One error for "every source failed": a specific, actionable type when any
/// source reported one, else the first error with a summary message.
fn aggregate_error(errors: &IndexMap<&'static str, RetrievalError>) -> Option<RetrievalError> {
    let (_, representative) = errors.first()?;
    let mut failed: Vec<&str> = errors.keys().copied().collect();
    failed.sort_unstable();
    let failed_sources = Value::Array(failed.iter().map(|&s| Value::String(s.into())).collect());
    for preferred in ["auth_required", "rate_limited", "malformed_response"] {
        if let Some(found) = errors.values().find(|e| e.error_type == preferred) {
            let mut error = found.clone();
            error
                .metadata
                .insert("failed_sources".into(), failed_sources);
            return Some(error);
        }
    }
    let mut metadata = Map::new();
    metadata.insert("failed_sources".into(), failed_sources);
    Some(RetrievalError {
        error_type: representative.error_type.clone(),
        message: format!("all sources failed ({})", failed.join(", ")),
        retryable: errors.values().any(|e| e.retryable),
        metadata,
    })
}
