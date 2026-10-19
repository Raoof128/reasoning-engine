//! Content-addressed evidence cache (plan Phase 8).
//!
//! [`CachedAdapter`] wraps one source: a search for the same source,
//! normalised query and limit within the source's TTL is served from the
//! cache instead of the network. Only successful results are cached; errors
//! always go back to the source. Cached records are rebound to the asking
//! run (`run_id`) but keep their original `retrieved_at`, so a run pack
//! still says when the evidence was actually fetched.
//!
//! Storage is behind [`CacheStore`] so this crate stays free of SQLite;
//! `re-verify` implements it over the engine database. Recorded runs
//! (`research --record`) never use the cache, because a hit would leave a
//! response off the tape.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use re_core::hashing::sha256_prefixed;
use re_model::records::EvidenceRecord;
use serde_json::{Map, Value};

use crate::aggregate::Adapter;
use crate::base::py_strip;
use crate::result::RetrievalResult;
use crate::sources::{BoxFuture, SourceAdapter};

/// arXiv lists new papers daily and revises often: six hours.
pub const ARXIV_TTL_SECS: i64 = 6 * 3600;
/// Every other source: a day.
pub const DEFAULT_TTL_SECS: i64 = 24 * 3600;

/// Where cached results live.
pub trait CacheStore: Send + Sync {
    /// The payload for `key` if it was stored at most its TTL before `now`.
    fn get(&self, key: &str, now: i64) -> Option<String>;
    /// Store `payload` (a JSON array of evidence records) under `key`.
    fn put(
        &self,
        key: &str,
        source: &str,
        query_norm: &str,
        payload: &str,
        ttl_secs: i64,
        now: i64,
    );
}

/// The TTL for `source`.
#[must_use]
pub fn ttl_secs(source: &str) -> i64 {
    if source == "arxiv" {
        ARXIV_TTL_SECS
    } else {
        DEFAULT_TTL_SECS
    }
}

/// Lower case, Python-stripped, inner whitespace collapsed.
#[must_use]
pub fn normalise_query(query: &str) -> String {
    py_strip(query)
        .split(re_core::pystr::py_is_space)
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// `sha256(source \n normalised query \n limit)`, hex.
#[must_use]
pub fn cache_key(source: &str, query: &str, limit: usize) -> String {
    let text = format!("{source}\n{}\n{limit}", normalise_query(query));
    sha256_prefixed(text.as_bytes())
        .trim_start_matches("sha256:")
        .to_string()
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// One source behind the cache.
pub struct CachedAdapter {
    inner: Adapter,
    store: Arc<dyn CacheStore>,
}

impl CachedAdapter {
    /// Wrap `inner`.
    #[must_use]
    pub fn new(inner: Adapter, store: Arc<dyn CacheStore>) -> Self {
        Self { inner, store }
    }
}

fn hit(run_id: &str, payload: &str) -> Option<RetrievalResult> {
    let mut evidence: Vec<EvidenceRecord> = serde_json::from_str(payload).ok()?;
    for record in &mut evidence {
        record.run_id = run_id.to_string();
    }
    let mut diagnostics = Map::new();
    diagnostics.insert("cache_hit".into(), Value::Bool(true));
    Some(RetrievalResult {
        evidence,
        error: None,
        diagnostics,
    })
}

impl SourceAdapter for CachedAdapter {
    fn name(&self) -> &'static str {
        self.inner.name()
    }

    fn search<'a>(
        &'a self,
        run_id: &'a str,
        query: &'a str,
        limit: usize,
    ) -> BoxFuture<'a, RetrievalResult> {
        Box::pin(async move {
            let source = self.inner.name();
            let key = cache_key(source, query, limit);
            if let Some(result) = self
                .store
                .get(&key, now_secs())
                .and_then(|payload| hit(run_id, &payload))
            {
                return result;
            }
            let mut result = self.inner.search(run_id, query, limit).await;
            if result.error.is_none()
                && let Ok(payload) = serde_json::to_string(&result.evidence)
            {
                self.store.put(
                    &key,
                    source,
                    &normalise_query(query),
                    &payload,
                    ttl_secs(source),
                    now_secs(),
                );
            }
            result
                .diagnostics
                .insert("cache_hit".into(), Value::Bool(false));
            result
        })
    }
}

/// Put every adapter behind `store`, except the offline mock and the
/// token-gated Scholar Gateway (they share the name `scholar_gateway`).
#[must_use]
pub fn cached(adapters: Vec<Adapter>, store: &Arc<dyn CacheStore>) -> Vec<Adapter> {
    adapters
        .into_iter()
        .map(|adapter| {
            if adapter.name() == crate::sources::mock::NAME {
                adapter
            } else {
                Arc::new(CachedAdapter::new(adapter, Arc::clone(store))) as Adapter
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use re_model::records::RetrievalError;

    #[derive(Default)]
    struct Memory(Mutex<HashMap<String, String>>);

    impl CacheStore for Memory {
        fn get(&self, key: &str, _: i64) -> Option<String> {
            self.0.lock().unwrap().get(key).cloned()
        }
        fn put(&self, key: &str, _: &str, _: &str, payload: &str, _: i64, _: i64) {
            self.0.lock().unwrap().insert(key.into(), payload.into());
        }
    }

    /// Fails first, then succeeds; counts calls.
    struct Flaky(AtomicUsize);

    impl SourceAdapter for Flaky {
        fn name(&self) -> &'static str {
            "crossref"
        }
        fn search<'a>(
            &'a self,
            run_id: &'a str,
            q: &'a str,
            _: usize,
        ) -> BoxFuture<'a, RetrievalResult> {
            let n = self.0.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                if n == 0 {
                    return RetrievalResult::failed(RetrievalError {
                        error_type: "rate_limited".into(),
                        message: "HTTP 429".into(),
                        retryable: true,
                        metadata: Map::new(),
                    });
                }
                RetrievalResult::ok(vec![crate::base::build_record(crate::base::RecordInput {
                    title: "t".into(),
                    ..crate::base::RecordInput::new(run_id, "crossref", q, 1)
                })])
            })
        }
    }

    #[tokio::test]
    async fn errors_are_not_cached_and_hits_are_rebound() {
        let flaky = Arc::new(Flaky(AtomicUsize::new(0)));
        let store: Arc<dyn CacheStore> = Arc::new(Memory::default());
        let adapter = CachedAdapter::new(Arc::clone(&flaky) as Adapter, Arc::clone(&store));
        assert!(adapter.search("run_a", "q", 5).await.error.is_some());
        let fetched = adapter.search("run_a", "q", 5).await;
        assert_eq!(
            fetched.diagnostics["cache_hit"], false,
            "the error was not cached"
        );
        let hit = adapter.search("run_b", " Q ", 5).await;
        assert_eq!(hit.diagnostics["cache_hit"], true);
        assert_eq!(hit.evidence[0].run_id, "run_b");
        assert_eq!(flaky.0.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn keys_ignore_case_and_spacing_but_not_source_or_limit() {
        assert_eq!(
            normalise_query("  Process  Reward\tModels "),
            "process reward models"
        );
        assert_eq!(
            cache_key("openalex", "Reward  models", 10),
            cache_key("openalex", "reward models", 10)
        );
        assert_ne!(
            cache_key("openalex", "reward models", 10),
            cache_key("crossref", "reward models", 10)
        );
        assert_ne!(
            cache_key("openalex", "reward models", 10),
            cache_key("openalex", "reward models", 5)
        );
        assert_eq!(ttl_secs("arxiv"), 21_600);
        assert_eq!(ttl_secs("doaj"), 86_400);
    }
}
