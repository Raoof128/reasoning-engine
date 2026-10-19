//! The adversarial angle through `plan_research_angles_tool` (plan Phase 10).
#![cfg(feature = "graph")]

use std::sync::Arc;

use re_mcp::Tools;
use re_retrieval::AcademicSearchAggregator;
use re_store::Store;
use re_verify::{LexicalOverlap, VerifiableResearchService};
use serde_json::{Map, Value, json};

/// The hardest golden query: a forest.
const HARD: &str = "Compare and contrast DPO versus PPO versus GRPO for RLHF; what are the \
    implications for reward hacking and how do they interact with process reward models?";

fn tools(dir: &std::path::Path) -> Tools {
    Tools::new(Arc::new(VerifiableResearchService::new(
        Store::open(dir.join("e.db")).unwrap(),
        dir.join("runs"),
        Arc::new(AcademicSearchAggregator::new(Vec::new())),
        Box::new(LexicalOverlap),
    )))
}

fn args(value: &Value) -> Map<String, Value> {
    value.as_object().cloned().unwrap()
}

#[tokio::test]
async fn hard_queries_end_with_an_adversarial_angle_when_asked() {
    let dir = tempfile::tempdir().unwrap();
    let request = args(&json!({"query": HARD, "max_angles": 4}));
    let legacy = tools(dir.path())
        .with_adversarial(false)
        .call("plan_research_angles_tool", &request)
        .await
        .unwrap();
    let angles = tools(dir.path())
        .with_adversarial(true)
        .call("plan_research_angles_tool", &request)
        .await
        .unwrap();
    let angles = angles.as_array().unwrap();
    assert_eq!(angles.len(), 4);
    assert_eq!(angles[..3], legacy.as_array().unwrap()[..3]);
    assert_eq!(angles[3]["name"], "adversarial");
    assert_eq!(angles[3]["priority"], 4);
    assert_eq!(
        angles[3]["starter_questions"][0],
        format!("evidence against {}", HARD.trim_end_matches('?'))
    );
}
