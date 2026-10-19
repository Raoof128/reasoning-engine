//! Phase 10 through the tools: the adversarial angle in
//! `plan_research_angles_tool`, and the one-off budget re-estimation in
//! `select_next_branches`.
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

async fn ok(tools: &Tools, name: &str, value: Value) -> Value {
    tools.call(name, &args(&value)).await.unwrap()
}

#[tokio::test]
async fn the_first_round_re_estimates_the_budget_once() {
    let dir = tempfile::tempdir().unwrap();
    let tools = tools(dir.path()).with_directional(true);
    let session = ok(
        &tools,
        "init_research_session",
        json!({"query": "what is a reward model"}),
    )
    .await;
    let sid = session["session_id"].as_str().unwrap().to_string();
    assert_eq!(session["strategy"], "single_pass", "{session:#}");
    let mut branches = Vec::new();
    for _ in 0..2 {
        let branch = ok(
            &tools,
            "register_branch",
            json!({"session_id": sid, "trace": ["t"]}),
        )
        .await;
        branches.push(branch["branch_id"].as_str().unwrap().to_string());
    }
    let before = ok(&tools, "get_session_state", json!({"session_id": sid})).await;
    let score = |bid: &str, q: f64| json!({"session_id": sid, "branch_id": bid, "q_score": q, "advantage": 0.1, "critique": "", "confidence": 0.5});
    // The branches disagree badly: one tier up, ten more steps.
    ok(&tools, "score_branch", score(&branches[0], 0.95)).await;
    ok(&tools, "score_branch", score(&branches[1], 0.1)).await;
    let plan = ok(&tools, "select_next_branches", json!({"session_id": sid})).await;
    let change = &plan["reestimate"];
    assert_eq!(change["from"], "single_pass", "{plan}");
    assert_eq!(change["to"], "best_of_n");
    assert_eq!(change["step_delta"], 10);
    let keys: Vec<&String> = plan.as_object().unwrap().keys().collect();
    assert_eq!(
        keys[keys.len() - 2..],
        ["reestimate", "directions"],
        "directions stay last"
    );
    let after = ok(&tools, "get_session_state", json!({"session_id": sid})).await;
    assert_eq!(after["session"]["strategy"], "best_of_n", "{after}");

    let again = ok(&tools, "select_next_branches", json!({"session_id": sid})).await;
    assert!(again.get("reestimate").is_none(), "once per session");
    assert!(before["session"].is_object());
}
