//! Aggregator behaviour (mirrors `tests/test_verifiable_aggregator.py`) and
//! ranking parity with the Python aggregator on the recorded fixtures.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use re_model::records::{EvidenceRecord, RetrievalError};
use re_retrieval::aggregate::Adapter;
use re_retrieval::base::{RecordInput, build_record};
use re_retrieval::sources::{BoxFuture, SourceConfig, free_source};
use re_retrieval::{AcademicSearchAggregator, RetrievalResult, SourceAdapter, adapters_from};
use serde_json::{Map, Value, json};

/// Returns a fixed result, like Python's `FakeAdapter`.
struct Fake {
    name: &'static str,
    result: RetrievalResult,
}

impl SourceAdapter for Fake {
    fn name(&self) -> &'static str {
        self.name
    }

    fn search<'a>(&'a self, _: &'a str, _: &'a str, _: usize) -> BoxFuture<'a, RetrievalResult> {
        let result = self.result.clone();
        Box::pin(async move { result })
    }
}

struct Crashing;

impl SourceAdapter for Crashing {
    fn name(&self) -> &'static str {
        "boom"
    }

    fn search<'a>(&'a self, _: &'a str, _: &'a str, _: usize) -> BoxFuture<'a, RetrievalResult> {
        Box::pin(async { panic!("kaboom") })
    }
}

fn fake(name: &'static str, result: RetrievalResult) -> Adapter {
    Arc::new(Fake { name, result })
}

fn rec(name: &'static str, doi: Option<&str>, title: &str, score: f64) -> EvidenceRecord {
    build_record(RecordInput {
        doi: doi.map_or(Value::Null, Value::from),
        title: title.into(),
        score,
        ..RecordInput::new("run_1", name, "q", 1)
    })
}

fn found(records: Vec<EvidenceRecord>) -> RetrievalResult {
    RetrievalResult::ok(records)
}

fn failed(error_type: &str, message: &str) -> RetrievalResult {
    RetrievalResult::failed(RetrievalError {
        error_type: error_type.into(),
        message: message.into(),
        retryable: true,
        metadata: Map::new(),
    })
}

async fn search(adapters: Vec<Adapter>, query: &str, limit: usize) -> RetrievalResult {
    AcademicSearchAggregator::new(adapters)
        .search("run_1", query, limit)
        .await
}

#[tokio::test]
async fn dedups_by_doi_and_keeps_higher_score() {
    let result = search(
        vec![
            fake(
                "openalex",
                found(vec![rec("openalex", Some("10.1/x"), "A", 0.9)]),
            ),
            fake(
                "crossref",
                found(vec![rec("crossref", Some("10.1/x"), "B", 0.4)]),
            ),
        ],
        "q",
        10,
    )
    .await;
    assert_eq!(result.evidence.len(), 1);
    assert_eq!(result.evidence[0].source_adapter, "openalex");
    assert_eq!(
        result.evidence[0].metadata["also_found_in"],
        json!(["crossref"])
    );
}

#[tokio::test]
async fn dedups_by_title_when_no_doi() {
    let result = search(
        vec![
            fake("arxiv", found(vec![rec("arxiv", None, "Same Title!", 0.3)])),
            fake("dblp", found(vec![rec("dblp", None, "same title", 0.7)])),
        ],
        "q",
        10,
    )
    .await;
    assert_eq!(result.evidence.len(), 1);
    assert_eq!(result.evidence[0].source_adapter, "dblp");
    assert_eq!(
        result.evidence[0].metadata["also_found_in"],
        json!(["arxiv"])
    );
}

#[tokio::test]
async fn title_keys_keep_combining_marks_out_like_python() {
    // U+093F is Other_Alphabetic (Rust alphanumeric) but not Python-isalnum.
    let result = search(
        vec![
            fake(
                "arxiv",
                found(vec![rec("arxiv", None, "\u{915}\u{93f}", 0.3)]),
            ),
            fake("dblp", found(vec![rec("dblp", None, "\u{915}", 0.7)])),
        ],
        "q",
        10,
    )
    .await;
    assert_eq!(result.evidence.len(), 1);
}

#[tokio::test]
async fn ranks_and_renumbers_evidence_ids() {
    let result = search(
        vec![
            fake(
                "openalex",
                found(vec![rec("openalex", Some("10.1/a"), "A", 0.2)]),
            ),
            fake(
                "crossref",
                found(vec![rec("crossref", Some("10.2/b"), "B", 0.8)]),
            ),
        ],
        "q",
        10,
    )
    .await;
    let ids: Vec<_> = result
        .evidence
        .iter()
        .map(|e| (e.evidence_id.as_str(), e.rank))
        .collect();
    assert_eq!(ids, [("ev_0001", 1), ("ev_0002", 2)]);
    assert_eq!(result.evidence[0].score.to_bits(), 0.8f64.to_bits());
}

#[tokio::test]
async fn truncates_to_limit() {
    let evidence = (0..10)
        .map(|i| {
            rec(
                "openalex",
                Some(&format!("10.1/{i}")),
                "T",
                1.0 - f64::from(i) * 0.01,
            )
        })
        .collect();
    let result = search(vec![fake("openalex", found(evidence))], "q", 3).await;
    assert_eq!(result.evidence.len(), 3);
}

#[tokio::test]
async fn tolerates_partial_failure() {
    let result = search(
        vec![
            fake(
                "openalex",
                found(vec![rec("openalex", Some("10.1/x"), "A", 0.5)]),
            ),
            fake("crossref", failed("unavailable", "down")),
        ],
        "q",
        10,
    )
    .await;
    assert_eq!(result.error, None);
    assert_eq!(result.evidence.len(), 1);
}

#[tokio::test]
async fn surfaces_actionable_error_when_all_fail() {
    let result = search(
        vec![
            fake("openalex", failed("rate_limited", "429")),
            fake("crossref", failed("unavailable", "down")),
        ],
        "q",
        10,
    )
    .await;
    let error = result.error.expect("error");
    assert_eq!(error.error_type, "rate_limited");
    assert_eq!(error.message, "429");
    assert_eq!(
        error.metadata["failed_sources"],
        json!(["crossref", "openalex"])
    );
}

#[tokio::test]
async fn summarises_generic_failures() {
    let result = search(
        vec![
            fake("openalex", failed("unavailable", "a")),
            fake("crossref", failed("empty_result", "b")),
        ],
        "q",
        10,
    )
    .await;
    let error = result.error.expect("error");
    assert_eq!(error.error_type, "unavailable");
    assert_eq!(error.message, "all sources failed (crossref, openalex)");
    assert!(error.retryable);
}

#[tokio::test]
async fn rejects_empty_query_and_no_adapters() {
    let result = search(vec![fake("x", found(Vec::new()))], " \u{1f} ", 10).await;
    assert_eq!(result.error.expect("error").error_type, "unsupported_query");
    let result = search(Vec::new(), "q", 10).await;
    assert_eq!(
        result.error.expect("error").error_type,
        "adapter_misconfigured"
    );
}

#[tokio::test]
async fn survives_a_panicking_adapter() {
    let only_crash = search(vec![Arc::new(Crashing)], "q", 10).await;
    let error = only_crash.error.expect("error");
    assert_eq!(
        (error.error_type.as_str(), error.retryable),
        ("unavailable", true)
    );
    assert_eq!(error.message, "all sources failed (boom)");

    let good = fake(
        "openalex",
        found(vec![rec("openalex", Some("10.1/x"), "A", 0.5)]),
    );
    let result = search(vec![Arc::new(Crashing), good], "q", 10).await;
    assert_eq!(result.evidence.len(), 1);
}

#[tokio::test]
async fn panics_report_their_own_adapter() {
    let mut adapters: Vec<Adapter> = vec![Arc::new(Crashing)];
    adapters.extend((0..12).map(|_| fake("crossref", failed("unavailable", "down"))));
    let result = search(adapters, "q", 10).await;
    assert_eq!(
        result.error.expect("error").metadata["failed_sources"],
        json!(["boom", "crossref"])
    );
}

fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
    let map: HashMap<String, String> = pairs
        .iter()
        .map(|(k, v)| ((*k).into(), (*v).into()))
        .collect();
    move |var| map.get(var).cloned()
}

fn names(adapters: &[Adapter]) -> Vec<&'static str> {
    adapters.iter().map(|a| a.name()).collect()
}

#[test]
fn default_adapters_offline_returns_mock() {
    let adapters = adapters_from(env(&[("REASONING_ENGINE_OFFLINE", "1")])).expect("adapters");
    assert_eq!(names(&adapters), ["scholar_gateway"]);
}

#[test]
fn default_adapters_honour_source_selection() {
    let adapters = adapters_from(env(&[(
        "REASONING_ENGINE_SOURCES",
        " crossref, nope,,openalex ",
    )]))
    .expect("adapters");
    assert_eq!(names(&adapters), ["crossref", "openalex"]);
}

#[test]
fn default_adapters_default_to_all_free_in_python_order() {
    let adapters = adapters_from(env(&[])).expect("adapters");
    assert_eq!(
        names(&adapters),
        [
            "openalex",
            "crossref",
            "arxiv",
            "semantic_scholar",
            "europepmc",
            "doaj",
            "dblp"
        ]
    );
}

#[cfg(feature = "scholar-gateway")]
#[test]
fn scholar_gateway_joins_only_when_configured() {
    let live = [
        ("SCHOLAR_GATEWAY_LIVE", "1"),
        ("SCHOLAR_GATEWAY_ACCESS_TOKEN", "t"),
    ];
    assert_eq!(adapters_from(env(&live)).expect("adapters").len(), 8);
    let selected = [live[0], live[1], ("REASONING_ENGINE_SOURCES", "dblp")];
    assert_eq!(
        names(&adapters_from(env(&selected)).expect("adapters")),
        ["dblp"]
    );
    assert_eq!(adapters_from(env(&live[..1])).expect("adapters").len(), 7);
}

fn http_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/http")
}

fn without(mut value: Value, key: &str) -> Value {
    if let Some(map) = value.as_object_mut() {
        map.remove(key);
    }
    value
}

/// Python fed each fixture case's adapter result to the real aggregator;
/// the Rust aggregator must rank, dedup and re-id identically.
#[tokio::test]
async fn ranking_matches_python_on_recorded_fixtures() {
    let text = std::fs::read_to_string(http_dir().join("expected.json")).expect("expected.json");
    let expected: Value = serde_json::from_str(&text).expect("valid JSON");
    let cases = &expected["cases"];
    let aggregates = expected["aggregates"].as_object().expect("aggregates");
    assert_eq!(aggregates.len(), 3);
    for (label, vector) in aggregates {
        let adapters: Vec<Adapter> = vector["keys"]
            .as_array()
            .expect("keys")
            .iter()
            .map(|key| {
                let case = &cases[key.as_str().expect("key")];
                let source = free_source(
                    case["source"].as_str().expect("source"),
                    &SourceConfig::default(),
                )
                .expect("known source");
                let body = std::fs::read(http_dir().join(case["body"].as_str().expect("body")))
                    .expect("body");
                let limit = usize::try_from(case["limit"].as_u64().expect("limit")).expect("small");
                let result = source.parse_body(
                    "run_fixture",
                    case["query"].as_str().expect("query"),
                    limit,
                    &body,
                );
                fake(source.name(), result)
            })
            .collect();
        let limit = usize::try_from(vector["limit"].as_u64().expect("limit")).expect("small");
        let result = AcademicSearchAggregator::new(adapters)
            .search("run_fixture", "q", limit)
            .await;
        let evidence: Vec<Value> = result
            .evidence
            .iter()
            .map(|e| without(e.to_dict(), "retrieved_at"))
            .collect();
        assert_eq!(
            Value::Array(evidence),
            vector["evidence"],
            "{label}: evidence"
        );
        let error = result.error.map_or(Value::Null, |e| {
            serde_json::to_value(e).expect("serialises")
        });
        assert_eq!(
            without(error, "message"),
            without(vector["error"].clone(), "message"),
            "{label}: error"
        );
    }
}

/// Answers after `delay`, like a slow provider.
struct Slow {
    name: &'static str,
    delay: std::time::Duration,
    result: RetrievalResult,
}

impl SourceAdapter for Slow {
    fn name(&self) -> &'static str {
        self.name
    }

    fn search<'a>(&'a self, _: &'a str, _: &'a str, _: usize) -> BoxFuture<'a, RetrievalResult> {
        let (delay, result) = (self.delay, self.result.clone());
        Box::pin(async move {
            tokio::time::sleep(delay).await;
            result
        })
    }
}

fn slow(name: &'static str, millis: u64, result: RetrievalResult) -> Adapter {
    Arc::new(Slow {
        name,
        delay: std::time::Duration::from_millis(millis),
        result,
    })
}

fn policy(quorum: usize, grace_ms: u64, deadline_ms: u64) -> re_retrieval::aggregate::FanoutPolicy {
    re_retrieval::aggregate::FanoutPolicy {
        quorum,
        grace: std::time::Duration::from_millis(grace_ms),
        deadline: std::time::Duration::from_millis(deadline_ms),
    }
}

#[tokio::test]
async fn a_slow_source_no_longer_sets_the_pace() {
    let aggregator = AcademicSearchAggregator::new(vec![
        fake(
            "fast_a",
            found(vec![rec("fast_a", Some("10.1/a"), "Alpha result", 0.9)]),
        ),
        fake(
            "fast_b",
            found(vec![rec("fast_b", Some("10.1/b"), "Beta result", 0.8)]),
        ),
        slow(
            "straggler",
            30_000,
            found(vec![rec("straggler", Some("10.1/c"), "Late", 0.99)]),
        ),
    ])
    .with_fanout(Some(policy(2, 50, 5_000)));
    let started = std::time::Instant::now();
    let result = aggregator.search("run_1", "q", 10).await;
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "waited for the straggler"
    );
    let sources: Vec<&str> = result
        .evidence
        .iter()
        .map(|e| e.source_adapter.as_str())
        .collect();
    assert_eq!(sources, ["fast_a", "fast_b"]);
    assert!(result.error.is_none());
    // The cut-off is on the record, not silent.
    let cut = result
        .diagnostics
        .get("straggler")
        .expect("straggler diagnostics");
    assert_eq!(cut["deadline_exceeded"], json!(true));
}

#[tokio::test]
async fn a_source_inside_the_grace_period_is_kept() {
    let aggregator = AcademicSearchAggregator::new(vec![
        fake(
            "fast",
            found(vec![rec("fast", Some("10.1/a"), "Alpha result", 0.9)]),
        ),
        slow(
            "a_bit_late",
            60,
            found(vec![rec("a_bit_late", Some("10.1/b"), "Beta", 0.8)]),
        ),
    ])
    .with_fanout(Some(policy(1, 2_000, 10_000)));
    let result = aggregator.search("run_1", "q", 10).await;
    assert_eq!(result.evidence.len(), 2);
    assert!(result.diagnostics.get("a_bit_late").is_none());
}

#[tokio::test]
async fn with_nothing_in_hand_the_deadline_does_not_return_empty() {
    // The only source with evidence is slower than the deadline. Returning
    // nothing on time would be worse than returning something late.
    let aggregator = AcademicSearchAggregator::new(vec![
        fake("empty", found(Vec::new())),
        slow(
            "only_one",
            150,
            found(vec![rec("only_one", Some("10.1/a"), "Alpha", 0.9)]),
        ),
    ])
    .with_fanout(Some(policy(1, 10, 20)));
    let result = aggregator.search("run_1", "q", 10).await;
    assert_eq!(result.evidence.len(), 1);
}

#[tokio::test]
async fn without_a_policy_every_source_is_awaited() {
    let aggregator = AcademicSearchAggregator::new(vec![
        fake(
            "fast",
            found(vec![rec("fast", Some("10.1/a"), "Alpha result", 0.9)]),
        ),
        slow(
            "late",
            120,
            found(vec![rec("late", Some("10.1/b"), "Beta", 0.8)]),
        ),
    ]);
    let result = aggregator.search("run_1", "q", 10).await;
    assert_eq!(result.evidence.len(), 2);
}

#[test]
fn fanout_policy_reads_the_environment() {
    use re_retrieval::aggregate::FanoutPolicy;
    let env = |value: Option<&'static str>| {
        move |var: &str| {
            (var == "REASONING_ENGINE_FANOUT_DEADLINE_SECS")
                .then(|| value.map(String::from))
                .flatten()
        }
    };
    assert_eq!(
        FanoutPolicy::from_env(env(None)),
        Some(FanoutPolicy::DEFAULT)
    );
    assert_eq!(FanoutPolicy::from_env(env(Some("0"))), None);
    assert_eq!(
        FanoutPolicy::from_env(env(Some("3"))).map(|p| p.deadline),
        Some(std::time::Duration::from_secs(3))
    );
    assert_eq!(
        FanoutPolicy::from_env(env(Some("soon"))),
        Some(FanoutPolicy::DEFAULT)
    );
}

#[tokio::test]
async fn one_paper_under_three_identifiers_takes_one_place() {
    // The publisher's DOI, the arXiv DOI and no DOI: three dedup keys, one work.
    let title = "MiniCheck: Efficient Fact-Checking of LLMs on Grounding Documents";
    let mut published = rec(
        "openalex",
        Some("10.18653/v1/2024.emnlp-main.499"),
        title,
        0.95,
    );
    published.year = Some(2024);
    let mut preprint = rec(
        "semantic_scholar",
        Some("10.48550/arxiv.2404.10774"),
        title,
        0.94,
    );
    preprint.year = Some(2024);
    preprint.snippet = "Recognizing if LLM output can be grounded in evidence.".into();
    let mut bare = rec(
        "arxiv",
        None,
        "MiniCheck: efficient fact-checking of LLMs on grounding documents.",
        0.6,
    );
    bare.year = Some(2024);
    let other = rec(
        "crossref",
        Some("10.1/other"),
        "A different paper about fact checking",
        0.5,
    );
    let aggregator = AcademicSearchAggregator::new(vec![
        fake("openalex", found(vec![published])),
        fake("semantic_scholar", found(vec![preprint])),
        fake("arxiv", found(vec![bare])),
        fake("crossref", found(vec![other])),
    ]);
    let result = aggregator.search("run_1", "q", 10).await;
    assert_eq!(result.evidence.len(), 2);
    let kept = &result.evidence[0];
    assert_eq!(kept.source_adapter, "openalex");
    assert_eq!(kept.doi.as_deref(), Some("10.18653/v1/2024.emnlp-main.499"));
    // The best record had no abstract; it takes the preprint's.
    assert_eq!(
        kept.snippet,
        "Recognizing if LLM output can be grounded in evidence."
    );
    assert_eq!(
        kept.metadata["also_found_in"],
        json!(["semantic_scholar", "arxiv"])
    );
    // A borrowed field names the source it came from; the keeper's own do not.
    assert_eq!(kept.metadata["snippet_from"], json!("semantic_scholar"));
    assert!(!kept.metadata.contains_key("doi_from"));
}

#[tokio::test]
async fn a_borrowed_field_keeps_its_origin_through_a_second_merge() {
    // `first` borrows the abstract from `second`; `best` then outranks both
    // and takes that abstract. Its origin is still `second`, not `first`.
    let title = "Sentence-level evidence pooling for claim verification";
    let first = rec("a", None, title, 0.7);
    let mut second = rec("b", Some("10.1/b"), title, 0.6);
    second.snippet = "Pooling entailment over sentences raises recall.".into();
    let best = rec("c", None, title, 0.9);
    let aggregator = AcademicSearchAggregator::new(vec![
        fake("a", found(vec![first])),
        fake("b", found(vec![second])),
        fake("c", found(vec![best])),
    ]);
    let result = aggregator.search("run_1", "q", 10).await;
    assert_eq!(result.evidence.len(), 1);
    let kept = &result.evidence[0];
    assert_eq!(kept.source_adapter, "c");
    assert_eq!(kept.metadata["snippet_from"], json!("b"));
    assert_eq!(kept.metadata["doi_from"], json!("b"));
    assert_eq!(kept.doi.as_deref(), Some("10.1/b"));
}

#[tokio::test]
async fn one_title_by_different_authors_is_two_works() {
    let title = "Towards robust detection of machine-generated text";
    let mut one = rec("a", Some("10.1/a"), title, 0.9);
    one.authors = vec!["Okafor, N.".into(), "Lindqvist, P.".into()];
    let mut two = rec("b", Some("10.1/b"), title, 0.8);
    two.authors = vec!["Mei Tanaka".into()];
    two.snippet = "A different study with the same name.".into();
    // The same people written another way are still one work.
    let mut again = rec("c", Some("10.1/c"), title, 0.7);
    again.authors = vec!["Nkem Okafor".into(), "Per Lindqvist".into()];
    let aggregator = AcademicSearchAggregator::new(vec![
        fake("a", found(vec![one])),
        fake("b", found(vec![two])),
        fake("c", found(vec![again])),
    ]);
    let result = aggregator.search("run_1", "q", 10).await;
    assert_eq!(result.evidence.len(), 2);
    let kept = &result.evidence[0];
    assert_eq!(kept.source_adapter, "a");
    assert_eq!(kept.metadata["also_found_in"], json!(["c"]));
    assert_eq!(kept.snippet, "", "no abstract taken from another work");
}

#[tokio::test]
async fn generic_or_distant_titles_are_not_merged() {
    let mut early = rec(
        "a",
        Some("10.1/a"),
        "A survey of natural language inference methods",
        0.9,
    );
    early.year = Some(2015);
    let mut late = rec(
        "b",
        Some("10.1/b"),
        "A survey of natural language inference methods",
        0.8,
    );
    late.year = Some(2024);
    let aggregator = AcademicSearchAggregator::new(vec![
        fake(
            "a",
            found(vec![early, rec("a", Some("10.1/c"), "Editorial", 0.3)]),
        ),
        fake(
            "b",
            found(vec![late, rec("b", Some("10.1/d"), "Editorial", 0.2)]),
        ),
    ]);
    let result = aggregator.search("run_1", "q", 10).await;
    assert_eq!(result.evidence.len(), 4);
}

#[tokio::test]
async fn the_last_straggler_gets_only_the_grace_period_below_quorum() {
    // Two sources with evidence, two empty, one slow: the quorum of three is
    // never met, but nothing else is coming, so the deadline is not waited out.
    let aggregator = AcademicSearchAggregator::new(vec![
        fake(
            "a",
            found(vec![rec("a", Some("10.1/a"), "Alpha result", 0.9)]),
        ),
        fake(
            "b",
            found(vec![rec("b", Some("10.1/b"), "Beta result", 0.8)]),
        ),
        fake("empty_1", found(Vec::new())),
        fake("empty_2", found(Vec::new())),
        slow(
            "straggler",
            30_000,
            found(vec![rec("straggler", Some("10.1/c"), "Late", 0.7)]),
        ),
    ])
    .with_fanout(Some(policy(3, 50, 20_000)));
    let started = std::time::Instant::now();
    let result = aggregator.search("run_1", "q", 10).await;
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
    assert_eq!(result.evidence.len(), 2);
}
