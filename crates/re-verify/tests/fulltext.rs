//! The pipeline reading open-access full text (plan Phase 9): findings in
//! the methods or results support a claim, background in the introduction
//! only partly does, and what was read is in provenance.
#![cfg(feature = "fulltext")]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use re_model::records::QualityHints;
use re_retrieval::base::{RecordInput, build_record};
use re_retrieval::fulltext::FullTextFetcher;
use re_retrieval::sources::BoxFuture;
use re_retrieval::{AcademicSearchAggregator, HttpClient, RetrievalResult, SourceAdapter};
use re_store::Store;
use re_verify::{LexicalOverlap, VerifiableResearchService};
use serde_json::Value;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// From the methods section, not the abstract.
const METHODS: &str = "A total of 23 participants were recruited between April 2019 and July 2019.";
/// From the introduction, citing earlier work.
const BACKGROUND: &str = "Extending nighttime sleep has been shown to increase performance, \
    support optimal hormonal responses and adaptation in professional rugby union athletes.";

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/fulltext")
        .join(name)
}

/// One Europe PMC record for the napping trial, its snippet the abstract.
struct Provider;

impl SourceAdapter for Provider {
    fn name(&self) -> &'static str {
        "europepmc"
    }

    fn search<'a>(
        &'a self,
        run_id: &'a str,
        query: &'a str,
        _: usize,
    ) -> BoxFuture<'a, RetrievalResult> {
        Box::pin(async move {
            let mut record = build_record(RecordInput {
                title: "Daytime naps improve afternoon power in elite rugby union athletes".into(),
                snippet: "Daytime naps are used by elite athletes in both training and \
                    match-day settings."
                    .into(),
                score: 0.9,
                ..RecordInput::new(run_id, "europepmc", query, 1)
            });
            record.hints = QualityHints {
                pmcid: Some("PMC10710987".into()),
                ..QualityHints::default()
            };
            RetrievalResult::ok(vec![record])
        })
    }
}

async fn europepmc() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/PMC10710987/fullTextXML"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(std::fs::read_to_string(fixture("PMC10710987.xml")).unwrap()),
        )
        .mount(&server)
        .await;
    server
}

#[tokio::test]
async fn claims_are_verified_against_the_paper_body() {
    let server = europepmc().await;
    let dir = tempfile::tempdir().unwrap();
    let fetcher =
        FullTextFetcher::new(HttpClient::new().unwrap()).with_europepmc_base(server.uri());
    let service = VerifiableResearchService::new(
        Store::open(dir.path().join("e.db")).unwrap(),
        dir.path().join("runs"),
        Arc::new(AcademicSearchAggregator::new(vec![Arc::new(Provider)])),
        Box::new(LexicalOverlap),
    )
    .with_full_text(fetcher);
    let result = service
        .run_research_pipeline(
            "daytime naps rugby",
            &format!("{METHODS} {BACKGROUND}"),
            "standard",
            "general",
            false,
        )
        .await
        .unwrap();
    let run_id = result["run_id"].as_str().unwrap();
    let pack = Path::new(result["run_pack"].as_str().unwrap());
    let verifications: Vec<Value> = serde_json::from_str(
        &std::fs::read_to_string(pack.join("claim_verifications.json")).unwrap(),
    )
    .unwrap();

    let methods = &verifications[0];
    assert_eq!(methods["support_status"], "supported", "{methods}");
    assert_eq!(methods["evidence_ids"], serde_json::json!(["ev_0001"]));
    assert!(
        methods["support_rationale"]
            .as_str()
            .unwrap()
            .ends_with("Full text: methods."),
        "{methods}"
    );

    let background = &verifications[1];
    assert_eq!(
        background["support_status"], "partially_supported",
        "{background}"
    );
    assert_eq!(background["requires_human_review"], true);
    assert!(
        background["support_rationale"]
            .as_str()
            .unwrap()
            .contains("only from the introduction"),
        "{background}"
    );

    let event = service
        .store()
        .list_provenance(run_id)
        .unwrap()
        .into_iter()
        .find(|e| e["event_type"] == "full_text")
        .expect("a full_text event");
    let document = &event["documents"][0];
    assert_eq!(document["evidence_id"], "ev_0001");
    assert_eq!(document["format"], "jats");
    assert_eq!(
        document["sections"],
        serde_json::json!([
            "abstract",
            "other",
            "introduction",
            "methods",
            "results",
            "discussion",
            "conclusion"
        ])
    );
    assert!(document["chunks"].as_u64().unwrap() > 20);
}
