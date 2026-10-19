//! Record a run over real recorded provider bodies, replay it offline, and
//! check that replay is byte-identical and that tampering is caught.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use re_retrieval::sources::{FREE_SOURCES, SourceConfig, free_source};
use re_retrieval::tape::{Outcome, Recording, Tape, redact_url, request_key};
use re_retrieval::{AcademicSearchAggregator, adapters_with};
use re_store::Store;
use re_verify::replay::{RecordPlan, replay_pack, run_recorded};
use re_verify::{LexicalOverlap, VerifiableResearchService};
use serde_json::Value;

const QUERY: &str = "reward model";
const DRAFT: &str =
    "Reward models score reasoning steps. Process reward models generalise to code.";

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/http")
}

/// The "network": each source's recorded body, keyed by the URL its adapter
/// builds for [`QUERY`] at the pipeline's limit.
fn network(dir: &Path) -> Arc<Tape> {
    write_network(dir);
    Arc::new(Tape::replaying(dir).unwrap())
}

fn write_network(dir: &Path) {
    let config = SourceConfig::default();
    std::fs::create_dir_all(dir).unwrap();
    for name in FREE_SOURCES {
        let source = free_source(name, &config).expect("free source");
        let url = source.build_url(QUERY, re_verify::service::PIPELINE_SEARCH_LIMIT);
        // DBLP served an anti-bot page when recording; use the hand-built body.
        let file = if name == "dblp" {
            "dblp__realistic".to_string()
        } else {
            name.to_string()
        };
        let body = std::fs::read_to_string(fixtures().join(format!("{file}.body"))).unwrap();
        let recording = Recording {
            method: "GET".into(),
            url: redact_url(&url),
            outcome: Outcome::Response {
                status: 200,
                headers: std::collections::BTreeMap::default(),
                body: Some(body),
                body_base64: None,
            },
        };
        let path = dir.join(format!("{}.json", request_key("GET", &url)));
        std::fs::write(path, serde_json::to_string(&recording).unwrap()).unwrap();
    }
}

async fn record(root: &Path) -> (PathBuf, Value) {
    record_with(root, false, false).await
}

/// The network plus full text: the top Europe PMC result gains a PMCID,
/// and it and the `OpenAlex` record's PDF URL serve the committed CC BY
/// documents.
#[cfg(feature = "fulltext")]
fn full_text_network(dir: &Path) -> Arc<Tape> {
    use base64::Engine as _;
    write_network(dir);
    let search = free_source("europepmc", &SourceConfig::default())
        .unwrap()
        .build_url(QUERY, re_verify::service::PIPELINE_SEARCH_LIMIT);
    let file = dir.join(format!("{}.json", request_key("GET", &search)));
    let mut recording: Recording =
        serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    if let Outcome::Response {
        body: Some(body), ..
    } = &mut recording.outcome
    {
        let mut json: Value = serde_json::from_str(body).unwrap();
        json["resultList"]["result"][0]["pmcid"] = "PMC12439617".into();
        *body = json.to_string();
    }
    std::fs::write(&file, serde_json::to_string(&recording).unwrap()).unwrap();
    let fixture = |name: &str| fixtures().join("../fulltext").join(name);
    let xml = std::fs::read_to_string(fixture("PMC10710987.xml")).unwrap();
    let pdf = std::fs::read(fixture("arxiv-2504.00406v2-p1-6.pdf")).unwrap();
    for (url, body, body_base64) in [
        (
            "https://www.ebi.ac.uk/europepmc/webservices/rest/PMC12439617/fullTextXML",
            Some(xml),
            None,
        ),
        (
            "https://ris.utwente.nl/ws/files/5489304/zapreev-mrmc.pdf",
            None,
            Some(base64::engine::general_purpose::STANDARD.encode(pdf)),
        ),
    ] {
        let recording = Recording {
            method: "GET".into(),
            url: redact_url(url),
            outcome: Outcome::Response {
                status: 200,
                headers: std::collections::BTreeMap::default(),
                body,
                body_base64,
            },
        };
        let path = dir.join(format!("{}.json", request_key("GET", url)));
        std::fs::write(path, serde_json::to_string(&recording).unwrap()).unwrap();
    }
    Arc::new(Tape::replaying(dir).unwrap())
}

async fn record_with(root: &Path, quality: bool, fulltext: bool) -> (PathBuf, Value) {
    #[cfg(feature = "fulltext")]
    let tape = if fulltext {
        full_text_network(&root.join("network"))
    } else {
        network(&root.join("network"))
    };
    #[cfg(not(feature = "fulltext"))]
    let tape = network(&root.join("network"));
    // The recording run has an API key; the replay below has none.
    let adapters = adapters_with(
        |var| (var == "OPENALEX_API_KEY").then(|| "SECRET-KEY".to_string()),
        Some(Arc::clone(&tape)),
    )
    .unwrap();
    let sources = adapters.iter().map(|a| a.name().to_string()).collect();
    let plan = RecordPlan {
        quality,
        fulltext,
        ..RecordPlan::now(sources, false, "lexical")
    };
    #[cfg(feature = "fulltext")]
    let text_tape = Arc::clone(&tape);
    let service = VerifiableResearchService::new(
        Store::open(root.join("record.db")).unwrap(),
        root.join("runs"),
        Arc::new(AcademicSearchAggregator::new(adapters)),
        Box::new(LexicalOverlap),
    )
    .with_recorder(plan.recorder(tape));
    let service = if quality {
        service.with_quality()
    } else {
        service
    };
    #[cfg(feature = "fulltext")]
    let service = if fulltext {
        let client = re_retrieval::HttpClient::new()
            .unwrap()
            .with_tape(text_tape);
        service.with_full_text(re_retrieval::fulltext::FullTextFetcher::new(client))
    } else {
        service
    };
    let result = run_recorded(&service, &plan, QUERY, DRAFT, "standard", "auto")
        .await
        .unwrap();
    let pack = PathBuf::from(result["run_pack"].as_str().unwrap());
    (pack, result)
}

#[tokio::test]
async fn a_recorded_run_replays_byte_for_byte_and_tampering_is_caught() {
    let root = tempfile::tempdir().unwrap();
    let (pack, result) = record(root.path()).await;
    assert!(
        result["evidence"].as_array().is_some_and(|e| e.len() > 3),
        "evidence from several sources: {}",
        result["evidence"]
    );
    assert_eq!(result["attestation"]["valid"], true);
    let http: Vec<_> = std::fs::read_dir(pack.join("http")).unwrap().collect();
    assert_eq!(http.len(), FREE_SOURCES.len());
    let everything: String = walk(&pack)
        .iter()
        .map(|p| std::fs::read_to_string(p).unwrap())
        .collect();
    assert!(
        !everything.contains("SECRET-KEY"),
        "API keys never reach a pack"
    );
    assert!(
        !everything.contains("example.org"),
        "contact addresses are redacted"
    );

    let report = replay_pack(&pack, &root.path().join("replay-1"), root.path(), true)
        .await
        .unwrap();
    assert_eq!(report["identical"], true, "{report}");
    assert_eq!(report["replayed_run_id"], report["run_id"]);
    assert_eq!(report["pack_attestation"]["valid"], true);

    // Tamper with one recorded body: the replay diverges, and the pack's own
    // attestation no longer verifies.
    let victim = std::fs::read_dir(pack.join("http"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| std::fs::read_to_string(p).unwrap().contains("openalex"))
        .expect("an openalex recording");
    let text = std::fs::read_to_string(&victim).unwrap();
    let tampered = text.replacen("\"title\\\": \\\"", "\"title\\\": \\\"Forged ", 1);
    assert_ne!(text, tampered, "the tamper must change the body");
    std::fs::write(&victim, tampered).unwrap();
    let report = replay_pack(&pack, &root.path().join("replay-2"), root.path(), true)
        .await
        .unwrap();
    assert_eq!(report["identical"], false);
    let mismatched = report["mismatched_artifacts"].to_string();
    assert!(mismatched.contains("evidence_ledger.json"), "{mismatched}");
    assert_eq!(report["pack_attestation"]["valid"], false);
    assert!(
        report["pack_attestation"]["mismatched_artifacts"]
            .to_string()
            .contains("http/")
    );
}

#[tokio::test]
async fn a_replay_never_reaches_the_network() {
    let root = tempfile::tempdir().unwrap();
    let (pack, _) = record(root.path()).await;
    // Drop every recording: each source now misses and fails closed.
    std::fs::remove_dir_all(pack.join("http")).unwrap();
    let report = replay_pack(&pack, &root.path().join("replay"), root.path(), true)
        .await
        .unwrap();
    assert_eq!(report["identical"], false);
    let provenance = std::fs::read_to_string(
        Path::new(report["replay_dir"].as_str().unwrap()).join("provenance.jsonl"),
    )
    .unwrap();
    assert!(
        provenance.contains("\"error_type\": \"replay_miss\""),
        "every source failed closed: {provenance}"
    );
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            out.extend(walk(&path));
        } else {
            out.push(path);
        }
    }
    out
}

#[tokio::test]
async fn quality_scoring_is_recorded_and_replayed() {
    let root = tempfile::tempdir().unwrap();
    let (plain, plain_result) = record_with(&root.path().join("plain"), false, false).await;
    let (pack, result) = record_with(&root.path().join("quality"), true, false).await;
    let settings = |pack: &Path| -> Value {
        serde_json::from_str(&std::fs::read_to_string(pack.join("replay.json")).unwrap()).unwrap()
    };
    assert_eq!(settings(&pack)["quality"], true);
    assert!(
        settings(&plain).get("quality").is_none(),
        "absent when off, as in older packs"
    );
    let scores = |result: &Value| -> Vec<Value> {
        result["evidence"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["score"].clone())
            .collect()
    };
    assert_ne!(scores(&result), scores(&plain_result), "quality rescored");

    let report = replay_pack(&pack, &root.path().join("replay"), root.path(), true)
        .await
        .unwrap();
    assert_eq!(report["identical"], true, "{report}");
}

#[cfg(feature = "fulltext")]
#[tokio::test]
async fn full_text_is_recorded_and_replayed() {
    let root = tempfile::tempdir().unwrap();
    let (pack, result) = record_with(root.path(), true, true).await;
    let settings: Value =
        serde_json::from_str(&std::fs::read_to_string(pack.join("replay.json")).unwrap()).unwrap();
    assert_eq!(settings["fulltext"], true);
    let provenance = std::fs::read_to_string(pack.join("provenance.jsonl")).unwrap();
    let event: Value = provenance
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .find(|e| e["event_type"] == "full_text")
        .expect("a full_text event");
    let formats: Vec<&str> = event["documents"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["format"].as_str().unwrap())
        .collect();
    assert_eq!(formats.len(), 2, "{event:#}");
    assert!(
        formats.contains(&"jats") && formats.contains(&"pdf"),
        "{event}"
    );
    assert_eq!(result["attestation"]["valid"], true);

    let report = replay_pack(&pack, &root.path().join("replay"), root.path(), true)
        .await
        .unwrap();
    assert_eq!(report["identical"], true, "{report}");
}
