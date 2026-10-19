//! Retrieval cost ledger and the database-backed evidence cache (plan
//! Phase 8).
//!
//! With the ledger on, every search appends a `retrieval_cost` provenance
//! event (bytes fetched, `OpenAlex` credits, cache hits, and the tokens the
//! result will cost a model, estimated as characters / 4), and a pipeline
//! run appends the `cost_ledger` totals before it exports, so the totals
//! land in `provenance.jsonl`.

use std::time::Duration;

use re_retrieval::cache::CacheStore;
use re_store::Store;
use serde_json::{Map, Value};

/// The engine database as a cache. Cache failures are never fatal: a read
/// error is a miss and a write error is dropped.
pub struct StoreCache(pub Store);

impl CacheStore for StoreCache {
    fn get(&self, key: &str, now: i64) -> Option<String> {
        self.0.cached_evidence(key, now).ok().flatten()
    }

    fn put(
        &self,
        key: &str,
        source: &str,
        query_norm: &str,
        payload: &str,
        ttl_secs: i64,
        now: i64,
    ) {
        let _ = self
            .0
            .cache_evidence(key, source, query_norm, payload, ttl_secs, now);
    }
}

fn number(value: &Value) -> f64 {
    value.as_f64().unwrap_or(0.0)
}

/// One search's cost from the aggregator's per-source diagnostics and the
/// size of what the search returns.
#[must_use]
pub fn search_cost(
    diagnostics: &Map<String, Value>,
    returned_chars: usize,
    elapsed: Duration,
) -> Map<String, Value> {
    let mut bytes = 0u64;
    let mut credits = 0.0;
    let mut hits = 0u64;
    let mut fetched = 0u64;
    for source in diagnostics.values().filter_map(Value::as_object) {
        bytes += source.get("bytes").and_then(Value::as_u64).unwrap_or(0);
        credits += source.get("openalex_credits_used").map_or(0.0, number);
        match source.get("cache_hit").and_then(Value::as_bool) {
            Some(true) => hits += 1,
            _ => fetched += 1,
        }
    }
    let mut cost = Map::new();
    cost.insert("bytes_fetched".into(), bytes.into());
    cost.insert("openalex_credits_used".into(), Value::from(credits));
    cost.insert("sources_fetched".into(), fetched.into());
    cost.insert("cache_hits".into(), hits.into());
    cost.insert("estimated_tokens".into(), (returned_chars / 4).into());
    cost.insert(
        "elapsed_ms".into(),
        u64::try_from(elapsed.as_millis())
            .unwrap_or(u64::MAX)
            .into(),
    );
    cost
}

/// Sum every `retrieval_cost` event in `provenance`.
#[must_use]
pub fn totals(provenance: &[Value]) -> Map<String, Value> {
    let mut out = Map::new();
    let mut searches = 0u64;
    for event in provenance
        .iter()
        .filter(|e| e.get("event_type").and_then(Value::as_str) == Some("retrieval_cost"))
    {
        searches += 1;
        for key in [
            "bytes_fetched",
            "openalex_credits_used",
            "sources_fetched",
            "cache_hits",
            "estimated_tokens",
            "elapsed_ms",
        ] {
            let sum = out.get(key).map_or(0.0, number) + event.get(key).map_or(0.0, number);
            out.insert(key.into(), Value::from(sum));
        }
    }
    out.insert("searches".into(), searches.into());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn costs_add_up_across_sources_and_searches() {
        let diagnostics = json!({
            "openalex": {"bytes": 1200, "openalex_credits_used": 1, "cache_hit": false},
            "crossref": {"bytes": 800, "cache_hit": false},
            "arxiv": {"cache_hit": true},
        });
        let cost = search_cost(
            diagnostics.as_object().unwrap(),
            400,
            Duration::from_millis(250),
        );
        assert_eq!(cost["bytes_fetched"], 2000);
        assert_eq!(cost["cache_hits"], 1);
        assert_eq!(cost["sources_fetched"], 2);
        assert_eq!(cost["estimated_tokens"], 100);
        let mut event = cost.clone();
        event.insert("event_type".into(), "retrieval_cost".into());
        let all = totals(&[
            Value::Object(event.clone()),
            Value::Object(event),
            json!({"event_type": "other"}),
        ]);
        assert_eq!(all["searches"], 2);
        assert_eq!(all["bytes_fetched"], 4000.0);
        assert_eq!(all["cache_hits"], 2.0);
    }
}
