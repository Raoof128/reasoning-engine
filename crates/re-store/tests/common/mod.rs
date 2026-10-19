//! Helpers for running the Python fixture script from Rust tests.
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

/// Run `tests/fixtures/make_python_db.py` against `db` and return its manifest.
pub fn make_python_db(db: &Path) -> Value {
    let script = repo_root().join("tests/fixtures/make_python_db.py");
    let output = Command::new(python())
        .arg("-I")
        .arg(&script)
        .arg(db)
        .output()
        .expect("fixture script runs");
    assert!(
        output.status.success(),
        "fixture script failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("fixture manifest is JSON")
}

/// `(type, name, tbl_name, sql)` for every schema object, with whitespace in
/// the SQL collapsed so indentation differences do not matter.
pub fn schema_of(db: &Path) -> Vec<(String, String, String, String)> {
    let conn = rusqlite::Connection::open(db).expect("open db");
    let mut stmt = conn
        .prepare(
            "SELECT type, name, tbl_name, COALESCE(sql, '') FROM sqlite_master ORDER BY type, name",
        )
        .expect("prepare");
    stmt.query_map([], |row| {
        let sql: String = row.get(3)?;
        Ok((
            row.get(0)?,
            row.get(1)?,
            row.get(2)?,
            sql.split_whitespace().collect::<Vec<_>>().join(" "),
        ))
    })
    .expect("query")
    .collect::<Result<_, _>>()
    .expect("rows")
}
