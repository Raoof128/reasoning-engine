//! Helpers for driving the Python verification code from Rust tests.
#![allow(dead_code, reason = "each test binary uses a different subset")]

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

/// Repository root.
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn python_version_ok(python: &str) -> bool {
    Command::new(python)
        .arg("-c")
        .arg("import sys; sys.exit(0 if sys.version_info >= (3, 11) else 1)")
        .status()
        .is_ok_and(|s| s.success())
}

/// A Python 3.11+ interpreter: `RE_PYTHON`, the repo `.venv`, then `python3`
/// and `python` on `PATH`. Panics with a clear message if none is found,
/// because these tests are the compatibility gate and must not skip silently.
pub fn python() -> String {
    let root = repo_root();
    let candidates = [
        std::env::var("RE_PYTHON").ok(),
        Some(root.join(".venv/bin/python").display().to_string()),
        Some(root.join(".venv/Scripts/python.exe").display().to_string()),
        Some("python3".to_string()),
        Some("python".to_string()),
    ];
    candidates
        .into_iter()
        .flatten()
        .find(|p| python_version_ok(p))
        .expect("Python 3.11+ is required for compatibility tests; set RE_PYTHON")
}

/// Run `tests/fixtures/verify_tool.py` with `args` and parse its JSON output.
pub fn verify_tool(args: &[&std::ffi::OsStr]) -> Value {
    let script = repo_root().join("tests/fixtures/verify_tool.py");
    let output = Command::new(python())
        .arg("-I")
        .arg(&script)
        .args(args)
        .output()
        .expect("verify_tool.py runs");
    assert!(
        output.status.success(),
        "verify_tool.py failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("verify_tool.py prints JSON")
}
