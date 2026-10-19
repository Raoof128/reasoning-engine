//! Phase 11 through the tools: a learned prior seeds new branches and
//! flags scores it disagrees with, the store turns scored sessions into
//! training data, and past evidence can be recalled with its last verdict.
#![cfg(feature = "learn")]

use std::sync::Arc;

use re_core::prior::{HASH_DIM, NUMERIC, Prior};
use re_mcp::Tools;
use re_retrieval::AcademicSearchAggregator;
use re_retrieval::sources::mock::MockScholarGateway;
use re_store::Store;
use re_verify::{LexicalOverlap, VerifiableResearchService};
use serde_json::{Map, Value, json};

fn service(dir: &std::path::Path) -> Arc<VerifiableResearchService> {
    Arc::new(
        VerifiableResearchService::new(
            Store::open(dir.join("e.db")).unwrap(),
            dir.join("runs"),
            Arc::new(AcademicSearchAggregator::new(vec![Arc::new(
                MockScholarGateway,
            )])),
            Box::new(LexicalOverlap),
        )
        .with_evidence_memory(),
    )
}

/// Predicts 0.9 for every branch, at full weight.
fn confident_prior() -> Prior {
    Prior {
        format: 1,
        weights: vec![0.0; HASH_DIM + NUMERIC],
        bias: (0.9_f64 / 0.1).ln(),
        sessions: 200,
        metrics: None,
    }
}

async fn ok(tools: &Tools, name: &str, value: Value) -> Value {
    let args: Map<String, Value> = value.as_object().cloned().unwrap();
    tools.call(name, &args).await.unwrap()
}

#[tokio::test]
async fn the_prior_seeds_branches_and_flags_disagreement() {
    let dir = tempfile::tempdir().unwrap();
    let tools = Tools::new(service(dir.path())).with_prior(confident_prior());
    let session = ok(
        &tools,
        "init_research_session",
        json!({"query": "reward models"}),
    )
    .await;
    let sid = session["session_id"].as_str().unwrap().to_string();
    let mut ids = Vec::new();
    for _ in 0..2 {
        let branch = ok(
            &tools,
            "register_branch",
            json!({"session_id": sid, "trace": ["check the primary evidence"]}),
        )
        .await;
        assert_eq!(branch["prior_score"], 0.9);
        ids.push(branch["branch_id"].as_str().unwrap().to_string());
    }
    let state = ok(&tools, "get_session_state", json!({"session_id": sid})).await;
    assert_eq!(
        state["branches"][0]["q_score"], 0.9,
        "seeded before scoring"
    );

    for (id, q) in ids.iter().zip([0.2, 0.8]) {
        ok(
            &tools,
            "score_branch",
            json!({"session_id": sid, "branch_id": id, "q_score": q, "advantage": 0.0, "critique": "", "confidence": 0.5}),
        )
        .await;
    }
    let plan = ok(&tools, "select_next_branches", json!({"session_id": sid})).await;
    let flagged = plan["prior_disagreement"].as_array().unwrap();
    assert_eq!(flagged.len(), 1, "{plan}");
    assert_eq!(flagged[0]["branch_id"], ids[0].as_str());
    assert_eq!(flagged[0]["prior"], 0.9);

    // That session is now training data: two scored branches, both in the
    // top three.
    let examples = Store::open(dir.path().join("e.db"))
        .unwrap()
        .training_examples()
        .unwrap();
    assert_eq!(examples.len(), 2);
    assert!(examples.iter().all(|e| e.in_consensus));
    assert_eq!(examples[0].features.trace, ["check the primary evidence"]);
}

#[tokio::test]
async fn without_a_prior_nothing_changes() {
    let dir = tempfile::tempdir().unwrap();
    let tools = Tools::new(service(dir.path()));
    let session = ok(
        &tools,
        "init_research_session",
        json!({"query": "reward models"}),
    )
    .await;
    let sid = session["session_id"].as_str().unwrap();
    let branch = ok(
        &tools,
        "register_branch",
        json!({"session_id": sid, "trace": ["t"]}),
    )
    .await;
    assert!(branch.get("prior_score").is_none());
    let plan = ok(&tools, "select_next_branches", json!({"session_id": sid})).await;
    assert!(plan.get("prior_disagreement").is_none());
}

#[tokio::test]
async fn past_evidence_is_recalled_with_its_last_verdict() {
    let dir = tempfile::tempdir().unwrap();
    let service = service(dir.path());
    let tools = Tools::new(Arc::clone(&service));
    service
        .run_research_pipeline(
            "MCP prompt injection defences",
            "Mock evidence for MCP prompt injection defences.",
            "standard",
            "auto",
            false,
        )
        .await
        .unwrap();
    let plain = ok(
        &tools,
        "recall_memory_tool",
        json!({"query": "prompt injection"}),
    )
    .await;
    assert!(plain.is_array(), "Python's shape by default");
    let recalled = ok(
        &tools,
        "recall_memory_tool",
        json!({"query": "prompt injection defences", "include_evidence": true}),
    )
    .await;
    assert!(recalled["memories"].is_array());
    let evidence = recalled["evidence"].as_array().unwrap();
    assert!(!evidence.is_empty(), "{recalled}");
    // The mock's records share one snippet, so which of them a tier cites
    // varies; one is cited and the rest are remembered as uncited.
    let statuses: Vec<&str> = evidence
        .iter()
        .map(|e| e["last_status"].as_str().unwrap())
        .collect();
    assert!(statuses.contains(&"supported"), "{recalled}");
    assert!(
        statuses
            .iter()
            .all(|s| ["supported", "uncited"].contains(s)),
        "{recalled}"
    );
    assert!(
        evidence[0]["last_run_id"]
            .as_str()
            .unwrap()
            .starts_with("run_")
    );
}
