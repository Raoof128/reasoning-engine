//! Tool definitions match the Python server's `tools/list` exactly.

use std::path::PathBuf;

use re_mcp::schema::{TOOLS, input_schema};
use serde_json::Value;

#[test]
fn tool_list_matches_python() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/mcp_tools.json");
    let python: Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("fixture")).expect("json");
    let python = python["tools"].as_array().expect("tools");
    assert_eq!(python.len(), TOOLS.len());
    for (spec, theirs) in TOOLS.iter().zip(python) {
        assert_eq!(spec.name, theirs["name"], "order");
        assert_eq!(spec.description, theirs["description"], "{}", spec.name);
        #[cfg_attr(not(any(feature = "tasks", feature = "learn")), allow(unused_mut))]
        let mut ours = input_schema(spec);
        // `include_evidence` is Rust-only (learn feature).
        #[cfg(feature = "learn")]
        if spec.name == "recall_memory_tool" {
            let flag = ours["properties"]
                .as_object_mut()
                .and_then(|p| p.remove("include_evidence"))
                .expect("include_evidence parameter");
            assert_eq!(flag["type"], "boolean");
            assert_eq!(flag["default"], false);
        }
        // `prefetch` is Rust-only (tasks feature); everything else matches.
        #[cfg(feature = "tasks")]
        if spec.name == "plan_research_angles_tool" {
            let prefetch = ours["properties"]
                .as_object_mut()
                .and_then(|p| p.remove("prefetch"))
                .expect("prefetch parameter");
            assert_eq!(prefetch["type"], "boolean");
            assert_eq!(prefetch["default"], false);
        }
        assert_eq!(Value::Object(ours), theirs["inputSchema"], "{}", spec.name);
    }
}
