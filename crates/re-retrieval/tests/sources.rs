//! Adapter parity with Python on recorded and edge-case responses.
//!
//! `tests/fixtures/http/expected.json` holds what the Python adapters
//! produced for each body (`record_http.py expected`). Every Rust adapter
//! must build the same URL and the same evidence; errors must agree on type,
//! retryability and metadata (messages come from different parsers).

use std::path::PathBuf;
use std::time::Duration;

use re_retrieval::sources::{
    HttpAdapter, HttpSource, SourceConfig, arxiv, crossref, dblp, doaj, europepmc, free_source,
    openalex, semantic_scholar,
};
use re_retrieval::{HttpClient, SourceAdapter};
use serde_json::Value;
use wiremock::matchers::{header, method};
use wiremock::{Mock, MockServer, ResponseTemplate};

const RUN_ID: &str = "run_fixture";

fn http_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/http")
}

fn expected() -> Value {
    let text = std::fs::read_to_string(http_dir().join("expected.json")).expect("expected.json");
    serde_json::from_str(&text).expect("valid JSON")
}

/// A source pointed at `endpoint`, with the default (keyless) config Python
/// ran under.
fn source_at(name: &str, endpoint: &str) -> Box<dyn HttpSource> {
    let config = SourceConfig::default();
    match name {
        "openalex" => Box::new(openalex::OpenAlex::new(&config).with_endpoint(endpoint)),
        "crossref" => Box::new(crossref::Crossref::new(&config).with_endpoint(endpoint)),
        "arxiv" => Box::new(arxiv::Arxiv::new().with_endpoint(endpoint)),
        "semantic_scholar" => {
            Box::new(semantic_scholar::SemanticScholar::new(&config).with_endpoint(endpoint))
        }
        "europepmc" => Box::new(europepmc::EuropePmc::new().with_endpoint(endpoint)),
        "doaj" => Box::new(doaj::Doaj::new().with_endpoint(endpoint)),
        "dblp" => Box::new(dblp::Dblp::new().with_endpoint(endpoint)),
        other => panic!("unknown source {other}"),
    }
}

fn evidence_dicts(result: &re_retrieval::RetrievalResult) -> Vec<Value> {
    result
        .evidence
        .iter()
        .map(|record| {
            let mut dict = record.to_dict();
            dict.as_object_mut().expect("object").remove("retrieved_at");
            dict
        })
        .collect()
}

fn comparable_error(error: &Value) -> Value {
    let mut error = error.clone();
    if let Some(map) = error.as_object_mut() {
        map.remove("message");
    }
    error
}

fn check(key: &str, case: &Value, result: &re_retrieval::RetrievalResult) {
    assert_eq!(
        evidence_dicts(result),
        case["evidence"].as_array().cloned().unwrap_or_default(),
        "{key}: evidence"
    );
    let error = result.error.as_ref().map_or(Value::Null, |e| {
        serde_json::to_value(e).expect("serialises")
    });
    assert_eq!(
        comparable_error(&error),
        comparable_error(&case["error"]),
        "{key}: error"
    );
}

#[test]
fn parse_body_matches_python_for_every_case() {
    let expected = expected();
    let cases = expected["cases"].as_object().expect("cases");
    assert!(cases.len() >= 20, "fixture set shrank to {}", cases.len());
    for (key, case) in cases {
        let name = case["source"].as_str().expect("source");
        let source = free_source(name, &SourceConfig::default()).expect("known source");
        let query = case["query"].as_str().expect("query");
        let limit = usize::try_from(case["limit"].as_u64().expect("limit")).expect("small");
        assert_eq!(
            source.build_url(query, limit),
            case["url"].as_str().expect("url"),
            "{key}: url"
        );
        let body = std::fs::read(http_dir().join(case["body"].as_str().expect("body")))
            .expect("body file");
        check(key, case, &source.parse_body(RUN_ID, query, limit, &body));
    }
}

#[test]
fn build_url_matches_python_for_awkward_queries() {
    let expected = expected();
    let urls = expected["urls"].as_array().expect("urls");
    assert_eq!(urls.len(), 21);
    for entry in urls {
        let name = entry["source"].as_str().expect("source");
        let source = free_source(name, &SourceConfig::default()).expect("known source");
        let limit = usize::try_from(entry["limit"].as_u64().expect("limit")).expect("small");
        let query = entry["query"].as_str().expect("query");
        assert_eq!(
            source.build_url(query, limit),
            entry["url"].as_str().expect("url"),
            "{name} {query:?}"
        );
    }
}

#[tokio::test]
async fn recorded_responses_round_trip_over_http() {
    let expected = expected();
    let client = HttpClient::with_timeout(Duration::from_secs(5)).expect("client");
    for (key, case) in expected["cases"].as_object().expect("cases") {
        if !key.ends_with("__recorded") {
            continue;
        }
        let server = MockServer::start().await;
        let body = std::fs::read(http_dir().join(case["body"].as_str().expect("body")))
            .expect("body file");
        Mock::given(method("GET"))
            .and(header("accept", "application/json"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(body))
            .expect(1)
            .mount(&server)
            .await;
        let name = case["source"].as_str().expect("source");
        let adapter = HttpAdapter::new(source_at(name, &server.uri()), client.clone());
        let query = case["query"].as_str().expect("query");
        let limit = usize::try_from(case["limit"].as_u64().expect("limit")).expect("small");
        check(key, case, &adapter.search(RUN_ID, query, limit).await);
    }
}

async fn fetch_with(response: ResponseTemplate) -> re_retrieval::RetrievalResult {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(response)
        .mount(&server)
        .await;
    let client = HttpClient::with_timeout(Duration::from_millis(500)).expect("client");
    let adapter = HttpAdapter::new(source_at("crossref", &server.uri()), client);
    adapter.search(RUN_ID, "q", 3).await
}

fn error_of(result: &re_retrieval::RetrievalResult) -> (&str, bool) {
    let error = result.error.as_ref().expect("an error");
    assert_eq!(error.metadata["source"], "crossref");
    (error.error_type.as_str(), error.retryable)
}

#[tokio::test]
async fn any_2xx_is_success() {
    let result =
        fetch_with(ResponseTemplate::new(202).set_body_string(r#"{"message": {"items": []}}"#))
            .await;
    assert_eq!(result.error, None);
}

#[tokio::test]
async fn http_errors_map_like_python() {
    assert_eq!(
        error_of(&fetch_with(ResponseTemplate::new(401)).await),
        ("auth_required", false)
    );
    assert_eq!(
        error_of(&fetch_with(ResponseTemplate::new(500)).await),
        ("unavailable", true)
    );
    let limited = fetch_with(ResponseTemplate::new(429).insert_header("retry-after", "7")).await;
    assert_eq!(error_of(&limited), ("rate_limited", true));
    assert_eq!(
        limited.error.expect("error").metadata["retry_after_secs"],
        7
    );
}

#[tokio::test]
async fn timeouts_are_retryable_unavailable() {
    let slow = ResponseTemplate::new(200).set_delay(Duration::from_secs(3));
    assert_eq!(error_of(&fetch_with(slow).await), ("unavailable", true));
}

#[tokio::test]
async fn non_json_bodies_are_malformed() {
    let result = fetch_with(ResponseTemplate::new(200).set_body_string("<html>")).await;
    assert_eq!(error_of(&result), ("malformed_response", false));
}

#[tokio::test]
async fn api_keys_travel_in_headers_and_credits_are_reported() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(header("x-api-key", "s2-secret"))
        .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"data": []}"#))
        .expect(1)
        .mount(&server)
        .await;
    let config = SourceConfig {
        semantic_scholar_api_key: Some("s2-secret".into()),
        ..SourceConfig::default()
    };
    let client = HttpClient::with_timeout(Duration::from_secs(5)).expect("client");
    let s2 = semantic_scholar::SemanticScholar::new(&config).with_endpoint(server.uri());
    let result = HttpAdapter::new(Box::new(s2), client.clone())
        .search(RUN_ID, "q", 3)
        .await;
    assert_eq!(result.error, None);

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(wiremock::matchers::query_param("api_key", "oa-secret"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("x-ratelimit-credits-used", "2")
                .set_body_string(r#"{"results": []}"#),
        )
        .expect(1)
        .mount(&server)
        .await;
    let config = SourceConfig {
        openalex_api_key: Some("oa-secret".into()),
        ..SourceConfig::default()
    };
    let oa = openalex::OpenAlex::new(&config).with_endpoint(server.uri());
    let result = HttpAdapter::new(Box::new(oa), client)
        .search(RUN_ID, "q", 3)
        .await;
    assert_eq!(result.diagnostics["openalex_credits_used"], 2.0);
}
