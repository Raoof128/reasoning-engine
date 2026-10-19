//! Speculative retrieval: prefetch answers at once, the work finishes in
//! the background, and the angle searches that follow are cache hits.
#![cfg(feature = "tasks")]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use re_mcp::Tools;
use re_retrieval::aggregate::Adapter;
use re_retrieval::base::{RecordInput, build_record};
use re_retrieval::cache::{CacheStore, cached};
use re_retrieval::sources::BoxFuture;
use re_retrieval::{AcademicSearchAggregator, RetrievalResult, SourceAdapter};
use re_store::Store;
use re_verify::ledger::StoreCache;
use re_verify::{LexicalOverlap, VerifiableResearchService};
use rmcp::model::TaskStatus;
use serde_json::{Map, Value, json};

/// A source that answers after `delay`, counting how often it is asked.
struct Counting {
    name: &'static str,
    delay: Duration,
    calls: Arc<AtomicUsize>,
}

impl SourceAdapter for Counting {
    fn name(&self) -> &'static str {
        self.name
    }

    fn search<'a>(
        &'a self,
        run_id: &'a str,
        query: &'a str,
        _: usize,
    ) -> BoxFuture<'a, RetrievalResult> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(self.delay).await;
            RetrievalResult::ok(vec![build_record(RecordInput {
                title: format!("{} result for {query}", self.name),
                snippet: format!("Evidence about {query}."),
                score: 0.5,
                ..RecordInput::new(run_id, self.name, query, 1)
            })])
        })
    }
}

fn args(value: &Value) -> Map<String, Value> {
    value.as_object().cloned().unwrap()
}

/// Tools over a fast and a 2 s source, both cached, with the ledger on.
fn fixture(dir: &std::path::Path) -> (Tools, Arc<AtomicUsize>, Arc<AtomicUsize>) {
    let fast_calls = Arc::new(AtomicUsize::new(0));
    let slow_calls = Arc::new(AtomicUsize::new(0));
    let adapters: Vec<Adapter> = vec![
        Arc::new(Counting {
            name: "openalex",
            delay: Duration::ZERO,
            calls: Arc::clone(&fast_calls),
        }),
        Arc::new(Counting {
            name: "crossref",
            delay: Duration::from_secs(2),
            calls: Arc::clone(&slow_calls),
        }),
    ];
    let cache: Arc<dyn CacheStore> = Arc::new(StoreCache(Store::open(dir.join("e.db")).unwrap()));
    let service = VerifiableResearchService::new(
        Store::open(dir.join("e.db")).unwrap(),
        dir.join("runs"),
        Arc::new(AcademicSearchAggregator::new(cached(adapters, &cache))),
        Box::new(LexicalOverlap),
    )
    .with_ledger();
    (Tools::new(Arc::new(service)), fast_calls, slow_calls)
}

#[tokio::test]
async fn prefetch_returns_at_once_and_later_searches_hit_the_cache() {
    let dir = tempfile::tempdir().unwrap();
    let (tools, fast_calls, slow_calls) = fixture(dir.path());

    let started = Instant::now();
    let planned = tools
        .call(
            "plan_research_angles_tool",
            &args(&json!({"query": "process reward models for code", "max_angles": 3, "prefetch": true})),
        )
        .await
        .unwrap();
    assert!(
        started.elapsed() < Duration::from_millis(500),
        "answers before the 2 s source"
    );
    let angles = planned["angles"].as_array().unwrap();
    assert_eq!(angles.len(), 3);
    let task_id = planned["prefetch"]["task_id"].as_str().unwrap().to_string();
    assert_eq!(
        planned["prefetch"]["queries"].as_array().unwrap().len(),
        4,
        "bare query plus 3 angles"
    );
    assert_eq!(
        tools.prefetcher().get(&task_id).unwrap().status(),
        TaskStatus::Working,
        "the slow source is still pending"
    );

    // The task finishes in the background.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let task = tools.prefetcher().get(&task_id).unwrap();
        if task.status() == TaskStatus::Completed {
            break;
        }
        assert!(Instant::now() < deadline, "prefetch did not finish");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let (fast, slow) = (
        fast_calls.load(Ordering::SeqCst),
        slow_calls.load(Ordering::SeqCst),
    );
    assert_eq!((fast, slow), (4, 4), "one search per query per source");

    // Angle 2, searched the way the skill does: no source is asked again.
    let run = tools
        .call(
            "start_research_run",
            &args(&json!({"query": "process reward models for code"})),
        )
        .await
        .unwrap();
    let run_id = run["run_id"].as_str().unwrap();
    let query = angles[1]["search_query"].as_str().unwrap();
    let started = Instant::now();
    let found = tools
        .call(
            "scholar_search_tool",
            &args(&json!({"run_id": run_id, "query": query})),
        )
        .await
        .unwrap();
    assert!(
        started.elapsed() < Duration::from_millis(500),
        "served from the cache"
    );
    assert_eq!(found["evidence"].as_array().unwrap().len(), 2);
    assert_eq!(
        found["evidence"][0]["run_id"], run_id,
        "rebound to the asking run"
    );
    assert_eq!(fast_calls.load(Ordering::SeqCst), 4);
    assert_eq!(slow_calls.load(Ordering::SeqCst), 4);

    // The ledger records the hits.
    let cost = re_store::Store::open(dir.path().join("e.db"))
        .unwrap()
        .list_provenance(run_id)
        .unwrap()
        .into_iter()
        .find(|e| e["event_type"] == "retrieval_cost")
        .expect("a retrieval_cost event");
    assert_eq!(cost["cache_hits"], 2);
    assert_eq!(cost["sources_fetched"], 0);
}

#[tokio::test]
async fn prefetch_is_off_by_default_and_bad_flags_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let service = VerifiableResearchService::new(
        Store::open(dir.path().join("e.db")).unwrap(),
        dir.path().join("runs"),
        Arc::new(AcademicSearchAggregator::new(Vec::new())),
        Box::new(LexicalOverlap),
    );
    let tools = Tools::new(Arc::new(service));
    let plain = tools
        .call(
            "plan_research_angles_tool",
            &args(&json!({"query": "reward models", "max_angles": 2})),
        )
        .await
        .unwrap();
    assert!(plain.is_array(), "unchanged Python shape without prefetch");
    let error = tools
        .call(
            "plan_research_angles_tool",
            &args(&json!({"query": "reward models", "prefetch": "maybe"})),
        )
        .await
        .unwrap_err();
    assert!(
        error.0.contains("Input should be a valid boolean"),
        "{}",
        error.0
    );
    let yes = tools
        .call(
            "plan_research_angles_tool",
            &args(&json!({"query": "reward models", "prefetch": "yes"})),
        )
        .await
        .unwrap();
    assert!(
        yes["prefetch"]["task_id"].is_string(),
        "pydantic-style lax booleans"
    );
}
