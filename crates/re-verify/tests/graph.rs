//! Phase 10 in the pipeline: contradiction search (a supported claim that a
//! counter-search contests is flagged and listed in the report's
//! counter-view; one that survives gains confidence) and conformal
//! prediction sets in provenance, and the evidence graph with its
//! retraction cascade.
#![cfg(feature = "graph")]

mod common;

use std::path::Path;
use std::sync::Arc;

use re_model::records::{ClaimRecord, EvidenceRecord, VerificationRecord};
use re_retrieval::base::{RecordInput, build_record};
use re_retrieval::sources::BoxFuture;
use re_retrieval::{AcademicSearchAggregator, RetrievalResult, SourceAdapter};
use re_store::Store;
use re_verify::calibration::Calibration;
use re_verify::{LexicalOverlap, VerifiableResearchService, Verifier};
use serde_json::Value;

const CONTESTED: &str = "Daytime naps improve afternoon power.";
const SURVIVOR: &str = "Daytime naps reduce muscle soreness.";
const CONTRA: &str = "No power benefit from napping was found.";

/// The pipeline's search finds both claims; the contradiction search for
/// the first finds a paper against it, for the second nothing.
struct Provider;

impl SourceAdapter for Provider {
    fn name(&self) -> &'static str {
        "openalex"
    }

    fn search<'a>(
        &'a self,
        run_id: &'a str,
        query: &'a str,
        _: usize,
    ) -> BoxFuture<'a, RetrievalResult> {
        let record = |rank: usize, title: &str, snippet: &str| {
            build_record(RecordInput {
                title: title.into(),
                snippet: snippet.into(),
                score: 0.9,
                ..RecordInput::new(run_id, "openalex", query, rank)
            })
        };
        let evidence = if query.starts_with("Daytime naps do not improve") {
            vec![record(1, "Naps show no power benefit", CONTRA)]
        } else if query.starts_with("Daytime naps do not") {
            Vec::new()
        } else {
            vec![
                record(1, "Napping and power", CONTESTED),
                record(2, "Napping and soreness", SURVIVOR),
            ]
        };
        Box::pin(async move { RetrievalResult::ok(evidence) })
    }
}

/// Supported when a snippet repeats the claim (ignoring case and the full
/// stop), contradicted by [`CONTRA`].
struct Scripted;

impl Verifier for Scripted {
    fn method(&self) -> &'static str {
        "scripted"
    }

    fn can_contradict(&self) -> bool {
        true
    }

    fn probabilities(&self, premise: &str, _: &str) -> Option<[f64; 3]> {
        Some(if premise == CONTRA {
            [0.05, 0.9, 0.05]
        } else {
            [0.75, 0.05, 0.2]
        })
    }

    fn verify(
        &self,
        claims: &mut [ClaimRecord],
        evidence: &[EvidenceRecord],
    ) -> Vec<VerificationRecord> {
        claims
            .iter_mut()
            .map(|claim| {
                let against: Vec<String> = evidence
                    .iter()
                    .filter(|e| e.snippet == CONTRA)
                    .map(|e| e.evidence_id.clone())
                    .collect();
                let support: Vec<String> = evidence
                    .iter()
                    .filter(|e| {
                        let core = |t: &str| t.trim().trim_end_matches('.').to_lowercase();
                        core(&e.snippet) == core(&claim.text)
                    })
                    .map(|e| e.evidence_id.clone())
                    .collect();
                let status = if !against.is_empty() {
                    "contradicted"
                } else if support.is_empty() {
                    "unsupported"
                } else {
                    "supported"
                };
                claim.status = status.into();
                claim.confidence = Some(0.8);
                VerificationRecord {
                    verification_id: format!("ver_{}", claim.claim_id),
                    claim_id: claim.claim_id.clone(),
                    method: "scripted".into(),
                    evidence_ids: support,
                    support_status: status.into(),
                    support_rationale: "Scripted.".into(),
                    missing_evidence: None,
                    contradictory_evidence_ids: against,
                    confidence: 0.8,
                    requires_human_review: status != "supported",
                    verified_at: "2026-10-19T00:00:00Z".into(),
                }
            })
            .collect()
    }
}

fn service(dir: &Path, verifier: Box<dyn Verifier>) -> VerifiableResearchService {
    VerifiableResearchService::new(
        Store::open(dir.join("e.db")).unwrap(),
        dir.join("runs"),
        Arc::new(AcademicSearchAggregator::new(vec![Arc::new(Provider)])),
        verifier,
    )
    .with_counter_search()
}

#[tokio::test]
async fn contested_claims_are_flagged_and_survivors_gain_confidence() {
    let dir = tempfile::tempdir().unwrap();
    let service = service(dir.path(), Box::new(Scripted));
    let result = service
        .run_research_pipeline(
            "daytime naps",
            &format!("{CONTESTED} {SURVIVOR}"),
            "standard",
            "general",
            false,
        )
        .await
        .unwrap();
    let pack = Path::new(result["run_pack"].as_str().unwrap());
    let verifications: Vec<Value> = serde_json::from_str(
        &std::fs::read_to_string(pack.join("claim_verifications.json")).unwrap(),
    )
    .unwrap();

    let contested = &verifications[0];
    assert_eq!(contested["support_status"], "supported");
    assert_eq!(contested["requires_human_review"], true);
    assert_eq!(contested["confidence"], 0.8);
    assert_eq!(
        contested["support_rationale"],
        "Scripted. Contradiction search found contrary work: Naps show no power benefit."
    );

    let survivor = &verifications[1];
    assert_eq!(survivor["confidence"], 0.85);
    assert_eq!(survivor["requires_human_review"], false);
    assert_eq!(
        survivor["support_rationale"],
        "Scripted. Survived contradiction search."
    );

    let report = std::fs::read_to_string(pack.join("report.md")).unwrap();
    assert!(
        report.ends_with(&format!(
            "## Counter-view\n\n- [supported, contested] {}: contrary: \
             Naps show no power benefit (openalex)\n",
            CONTESTED.trim_end_matches('.')
        )),
        "{report}"
    );

    let run_id = result["run_id"].as_str().unwrap();
    let event = service
        .store()
        .list_provenance(run_id)
        .unwrap()
        .into_iter()
        .find(|e| e["event_type"] == "counter_search")
        .expect("a counter_search event");
    let queries: Vec<&str> = event["searches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["query"].as_str().unwrap())
        .collect();
    assert_eq!(
        queries,
        [
            "Daytime naps do not improve afternoon power",
            "Daytime naps do not reduce muscle soreness"
        ]
    );
}

#[tokio::test]
async fn tiers_that_cannot_contradict_skip_the_search() {
    let dir = tempfile::tempdir().unwrap();
    let service = service(dir.path(), Box::new(LexicalOverlap));
    let result = service
        .run_research_pipeline("daytime naps", CONTESTED, "standard", "general", false)
        .await
        .unwrap();
    let run_id = result["run_id"].as_str().unwrap();
    assert!(
        service
            .store()
            .list_provenance(run_id)
            .unwrap()
            .iter()
            .all(|e| e["event_type"] != "counter_search")
    );
    let pack = Path::new(result["run_pack"].as_str().unwrap());
    assert!(
        !std::fs::read_to_string(pack.join("report.md"))
            .unwrap()
            .contains("Counter-view")
    );
}

#[tokio::test]
async fn verdicts_carry_conformal_prediction_sets() {
    let dir = tempfile::tempdir().unwrap();
    let service = service(dir.path(), Box::new(Scripted)).with_calibration(Calibration {
        alpha: 0.1,
        quantile: 0.3,
        n: 100,
        model: "scripted".into(),
    });
    let result = service
        .run_research_pipeline("daytime naps", SURVIVOR, "standard", "general", false)
        .await
        .unwrap();
    let run_id = result["run_id"].as_str().unwrap();
    let event = service
        .store()
        .list_provenance(run_id)
        .unwrap()
        .into_iter()
        .find(|e| e["event_type"] == "calibration")
        .expect("a calibration event");
    assert_eq!(event["coverage_level"], 0.9);
    let set = &event["verifications"][0];
    assert_eq!(set["label"], "supported");
    assert_eq!(set["prediction_set"], serde_json::json!(["supported"]));
    assert_eq!(set["in_prediction_set"], true);

    let plain = tempfile::tempdir().unwrap();
    let service = service_without_calibration(plain.path());
    let result = service
        .run_research_pipeline("daytime naps", SURVIVOR, "standard", "general", false)
        .await
        .unwrap();
    let run_id = result["run_id"].as_str().unwrap();
    assert!(
        service
            .store()
            .list_provenance(run_id)
            .unwrap()
            .iter()
            .all(|e| e["event_type"] != "calibration"),
        "nothing without a calibration"
    );
}

fn service_without_calibration(dir: &Path) -> VerifiableResearchService {
    service(dir, Box::new(Scripted))
}

/// Four papers: 1 cites 3, which is retracted; 2 and 4 share an author.
struct Linked;

const RETRACTED: &str = "A retracted napping study.";

impl SourceAdapter for Linked {
    fn name(&self) -> &'static str {
        "openalex"
    }

    fn search<'a>(
        &'a self,
        run_id: &'a str,
        query: &'a str,
        _: usize,
    ) -> BoxFuture<'a, RetrievalResult> {
        let record = |n: usize, snippet: &str| {
            let mut record = build_record(RecordInput {
                title: format!("Paper {n}"),
                snippet: snippet.into(),
                score: 1.0 - n as f64 / 10.0,
                ..RecordInput::new(run_id, "openalex", query, n)
            });
            record.doi = Some(format!("10.1/p{n}"));
            record.hints.retracted = (n == 3).then_some(true);
            record
        };
        Box::pin(async move {
            RetrievalResult::ok(vec![
                record(1, CONTESTED),
                record(2, SURVIVOR),
                record(3, RETRACTED),
                record(4, SURVIVOR),
            ])
        })
    }
}

#[tokio::test]
async fn the_graph_caps_claims_resting_on_retracted_work() {
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;
    let work = |n: u32, refs: &[&str], authors: &[&str]| {
        serde_json::json!({
            "id": format!("https://openalex.org/W{n}"),
            "doi": format!("https://doi.org/10.1/P{n}"),
            "referenced_works": refs.iter().map(|r| format!("https://openalex.org/{r}")).collect::<Vec<_>>(),
            "authorships": authors.iter().map(|a| serde_json::json!({"author": {"id": format!("https://openalex.org/{a}")}})).collect::<Vec<_>>(),
        })
    };
    Mock::given(method("GET"))
        .and(path("/works"))
        .and(query_param("select", "id,doi,referenced_works,authorships"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"results": [
                work(1, &["W3"], &["A1"]),
                work(2, &[], &["A2", "A7"]),
                work(3, &[], &["A3"]),
                work(4, &[], &["A7"]),
            ]})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let fetcher = re_retrieval::links::LinkFetcher::new(
        re_retrieval::HttpClient::new().unwrap(),
        &re_retrieval::sources::SourceConfig::default(),
    )
    .with_endpoint(format!("{}/works", server.uri()));
    let service = VerifiableResearchService::new(
        Store::open(dir.path().join("e.db")).unwrap(),
        dir.path().join("runs"),
        Arc::new(AcademicSearchAggregator::new(vec![Arc::new(Linked)])),
        Box::new(Scripted),
    )
    .with_evidence_graph(fetcher);
    let result = service
        .run_research_pipeline(
            "daytime naps",
            &format!("{CONTESTED} {SURVIVOR}"),
            "standard",
            "general",
            false,
        )
        .await
        .unwrap();
    let pack = Path::new(result["run_pack"].as_str().unwrap());
    let verifications: Vec<Value> = serde_json::from_str(
        &std::fs::read_to_string(pack.join("claim_verifications.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(verifications[0]["support_status"], "partially_supported");
    assert!(
        verifications[0]["support_rationale"]
            .as_str()
            .unwrap()
            .ends_with("rests on retracted work (ev_0003)."),
        "{}",
        verifications[0]
    );
    assert_eq!(verifications[1]["support_status"], "supported");

    let cascade: Value =
        serde_json::from_str(&std::fs::read_to_string(pack.join("cascade.json")).unwrap()).unwrap();
    assert_eq!(
        cascade[0]["paths"],
        serde_json::json!([["ev_0001", "ev_0003"]])
    );
    let attestation: Value =
        serde_json::from_str(&std::fs::read_to_string(pack.join("attestation.json")).unwrap())
            .unwrap();
    assert!(attestation["artifact_hashes"]["cascade.json"].is_string());

    let report = std::fs::read_to_string(pack.join("report.md")).unwrap();
    assert!(
        report.contains("2 supporting records, 1 independent\n\n```mermaid"),
        "{report}"
    );
    assert!(report.contains("ev_0002 ---|shared author| ev_0004"));

    let run_id = result["run_id"].as_str().unwrap();
    let event = service
        .store()
        .list_provenance(run_id)
        .unwrap()
        .into_iter()
        .find(|e| e["event_type"] == "evidence_graph")
        .expect("an evidence_graph event");
    assert_eq!(event["linked"], 4);
    assert_eq!(event["cascade"], 1);
    assert_eq!(event["retracted"], serde_json::json!(["ev_0003"]));

    let by_python = common::verify_tool(&[std::ffi::OsStr::new("verify"), pack.as_os_str()]);
    assert_eq!(by_python["valid"], true, "{by_python}");
}
