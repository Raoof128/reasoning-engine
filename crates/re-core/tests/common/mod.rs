//! Shared loader for the Python-generated golden vectors.
//!
//! The JSON is produced by `tests/golden/generate_golden.py` and must never
//! be edited by hand.
#![allow(dead_code, reason = "each test binary uses a different subset")]

use std::sync::OnceLock;

use serde_json::Value;

const GOLDEN_JSON: &str = include_str!("../../../../tests/golden/golden_vectors.json");

/// Parsed golden vectors, with key order preserved.
pub fn golden() -> &'static Value {
    static GOLDEN: OnceLock<Value> = OnceLock::new();
    GOLDEN.get_or_init(|| serde_json::from_str(GOLDEN_JSON).expect("golden vectors parse"))
}

/// The array stored under `key`.
pub fn section(key: &str) -> &'static Vec<Value> {
    golden()[key]
        .as_array()
        .unwrap_or_else(|| panic!("golden section {key} is not an array"))
}
