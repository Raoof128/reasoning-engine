//! A run replays with the snippet cap and the NLI model it was recorded
//! with, whatever the replaying process is set to. Both are settings for the
//! whole process, so this test has a binary to itself.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use re_retrieval::base::{LONG_SNIPPET_LIMIT, SNIPPET_LIMIT, set_snippet_limit, snippet_limit};
use re_retrieval::sources::{SourceConfig, free_source};
use re_retrieval::tape::{Outcome, Recording, Tape, redact_url, request_key};
use re_retrieval::{AcademicSearchAggregator, adapters_with};
use re_store::Store;
use re_verify::replay::{RecordPlan, replay_pack, run_recorded};
use re_verify::runpack::REPLAY_INPUTS;
use re_verify::verify::{nli_model, set_nli_model};
use re_verify::{LexicalOverlap, VerifiableResearchService};
use serde_json::{Value, json};

const QUERY: &str = "reward model";
const DRAFT: &str = "Process reward models generalise to code.";
const SOURCE: &str = "semantic_scholar";

fn only_this_source(var: &str) -> Option<String> {
    (var == "REASONING_ENGINE_SOURCES").then(|| SOURCE.to_string())
}

/// One paper whose abstract runs to about 3,000 characters and states its
/// conclusion at the end.
fn network(dir: &Path) -> Arc<Tape> {
    let filler = "Reward models score intermediate reasoning steps in many settings. ".repeat(44);
    let body = json!({"data": [{
        "paperId": "p1",
        "title": "Process reward models beyond mathematics",
        "abstract": format!("{filler}We conclude that process reward models generalise to code."),
        "year": 2025,
        "authors": [{"name": "A. Researcher"}],
        "externalIds": {"DOI": "10.1/prm"},
        "url": "https://example.com/prm",
    }]})
    .to_string();
    let source = free_source(SOURCE, &SourceConfig::default()).expect("a free source");
    let url = source.build_url(QUERY, re_verify::service::PIPELINE_SEARCH_LIMIT);
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
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join(format!("{}.json", request_key("GET", &url)));
    std::fs::write(path, serde_json::to_string(&recording).unwrap()).unwrap();
    Arc::new(Tape::replaying(dir).unwrap())
}

async fn record(root: &Path) -> (PathBuf, Value) {
    let tape = network(&root.join("network"));
    let adapters = adapters_with(only_this_source, Some(Arc::clone(&tape))).unwrap();
    let sources = adapters.iter().map(|a| a.name().to_string()).collect();
    let plan = RecordPlan::now(sources, false, "lexical");
    let service = VerifiableResearchService::new(
        Store::open(root.join("record.db")).unwrap(),
        root.join("runs"),
        Arc::new(AcademicSearchAggregator::new(adapters)),
        Box::new(LexicalOverlap),
    )
    .with_recorder(plan.recorder(tape));
    let result = run_recorded(&service, &plan, QUERY, DRAFT, "standard", "auto")
        .await
        .unwrap();
    (PathBuf::from(result["run_pack"].as_str().unwrap()), result)
}

fn inputs(pack: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(pack.join(REPLAY_INPUTS)).unwrap()).unwrap()
}

#[tokio::test]
async fn a_pack_replays_with_the_settings_it_was_recorded_with() {
    let root = tempfile::tempdir().unwrap();

    assert_eq!(nli_model(), "xsmall");
    assert!(
        set_nli_model("large").is_err(),
        "an unknown model is refused"
    );
    assert_eq!(nli_model(), "xsmall", "and changes nothing");

    // Recorded by a build that keeps 4,000 characters and uses the larger
    // NLI model.
    set_snippet_limit(LONG_SNIPPET_LIMIT);
    set_nli_model("base").unwrap();
    let (long_pack, result) = record(&root.path().join("long")).await;
    let snippet = result["evidence"][0]["snippet"].as_str().unwrap();
    assert!(
        snippet.chars().count() > SNIPPET_LIMIT,
        "the abstract is kept past 1,500"
    );
    assert!(
        snippet.ends_with("generalise to code."),
        "its conclusion included"
    );
    assert_eq!(inputs(&long_pack)["snippet_chars"], 4000);
    assert_eq!(inputs(&long_pack)["nli_model"], "base");

    // Recorded with the defaults: the pack mentions neither, as packs made
    // before the settings existed do not.
    set_snippet_limit(SNIPPET_LIMIT);
    set_nli_model("xsmall").unwrap();
    let (short_pack, result) = record(&root.path().join("short")).await;
    let snippet = result["evidence"][0]["snippet"].as_str().unwrap();
    assert_eq!(snippet.chars().count(), SNIPPET_LIMIT);
    assert!(inputs(&short_pack).get("snippet_chars").is_none());
    assert!(inputs(&short_pack).get("nli_model").is_none());

    // Each replays byte for byte from a process set the other way.
    let report = replay_pack(
        &long_pack,
        &root.path().join("replay-long"),
        root.path(),
        true,
    )
    .await
    .unwrap();
    assert_eq!(report["identical"], true, "{report}");
    assert_eq!(snippet_limit(), LONG_SNIPPET_LIMIT);
    assert_eq!(nli_model(), "base");

    let report = replay_pack(
        &short_pack,
        &root.path().join("replay-short"),
        root.path(),
        true,
    )
    .await
    .unwrap();
    assert_eq!(report["identical"], true, "{report}");
    assert_eq!(snippet_limit(), SNIPPET_LIMIT);
    assert_eq!(nli_model(), "xsmall");
}
