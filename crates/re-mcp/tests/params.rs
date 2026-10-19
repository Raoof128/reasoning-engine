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
        assert_eq!(
            Value::Object(input_schema(spec)),
            theirs["inputSchema"],
            "{}",
            spec.name
        );
    }
}
