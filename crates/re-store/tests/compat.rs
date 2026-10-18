//! A database written by the Python implementation must open unchanged, and
//! a fresh Rust database must have exactly Python's schema.

mod common;

use common::{make_python_db, schema_of};
use re_store::Store;
use re_store::schema::TABLES;

#[test]
fn opens_a_python_database_without_changing_it() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("python.db");
    let manifest = make_python_db(&db);
    let schema_before = schema_of(&db);

    let store = Store::open(&db).unwrap();

    for table in TABLES {
        let expected = manifest["rows"][table].as_array().unwrap().len();
        let expected = i64::try_from(expected).unwrap();
        assert_eq!(store.count_rows(table).unwrap(), expected, "table {table}");
    }
    assert_eq!(store.user_version().unwrap(), 2);
    drop(store);
    assert_eq!(
        schema_of(&db),
        schema_before,
        "opening must not alter the schema"
    );
}

#[test]
fn fresh_rust_database_has_python_schema() {
    let dir = tempfile::tempdir().unwrap();
    let py_db = dir.path().join("python.db");
    let rs_db = dir.path().join("rust.db");
    make_python_db(&py_db);
    Store::open(&rs_db).unwrap();
    assert_eq!(schema_of(&rs_db), schema_of(&py_db));
}

#[test]
fn reads_python_sessions_and_branches_exactly() {
    use re_store::sessions::{BranchRow, SessionRow};

    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("python.db");
    let manifest = make_python_db(&db);
    let store = Store::open(&db).unwrap();

    let session_id = manifest["session_id"].as_str().unwrap();
    let expected: SessionRow =
        serde_json::from_value(manifest["rows"]["sessions"][0].clone()).unwrap();
    assert_eq!(store.get_session(session_id).unwrap(), expected);

    for row in manifest["rows"]["branches"].as_array().unwrap() {
        let expected: BranchRow = serde_json::from_value(row.clone()).unwrap();
        assert_eq!(store.get_branch(&expected.id).unwrap(), expected);
    }
    let recalled = store.recall_memory("how do prms work", 5).unwrap();
    assert_eq!(recalled.len(), 1);
    assert_eq!(recalled[0].id, manifest["memory_id"].as_str().unwrap());
}

#[test]
fn writes_the_same_bytes_python_writes() {
    let dir = tempfile::tempdir().unwrap();
    let py_db = dir.path().join("python.db");
    let manifest = make_python_db(&py_db);
    let rows = &manifest["rows"];
    let store = Store::open(dir.path().join("rust.db")).unwrap();

    // Same query: same difficulty, strategy and budget columns.
    let session = store
        .create_session("How do PRMs interact with MCTS for test-time compute scaling?")
        .unwrap();
    let ours = serde_json::to_value(store.get_session(&session.id).unwrap()).unwrap();
    for column in [
        "difficulty",
        "strategy",
        "budget_total_branches",
        "budget_max_depth",
        "budget_max_steps",
        "budget_tokens_per_branch",
    ] {
        assert_eq!(ours[column], rows["sessions"][0][column], "{column}");
    }

    // Same trace input (non-ASCII, astral emoji, markup): same stored JSON text.
    let trace = [
        "Step 1: caf\u{e9} <b>bold</b>".to_string(),
        "Step 2: \u{646}\u{62a}\u{6cc}\u{62c}\u{647} \u{1f600}".to_string(),
    ];
    let branch = store
        .register_branch(&session.id, &trace, &[], None, 0)
        .unwrap();
    assert_eq!(
        store.get_branch(&branch.id).unwrap().trace,
        rows["branches"][0]["trace"].as_str().unwrap()
    );

    // Same memory input: same stored learnings and tags text.
    store
        .save_memory(
            &session.id,
            "How do PRMs work?",
            &[
                "PRMs score individual steps".into(),
                "Dense supervision \u{2011} beats ORM".into(),
            ],
            &["prm".into(), "reward-models".into()],
        )
        .unwrap();
    let conn = rusqlite::Connection::open(store.path()).unwrap();
    let (learnings, tags): (String, String) = conn
        .query_row(
            "SELECT key_learnings, domain_tags FROM episodic_memory",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        learnings,
        rows["episodic_memory"][0]["key_learnings"]
            .as_str()
            .unwrap()
    );
    assert_eq!(
        tags,
        rows["episodic_memory"][0]["domain_tags"].as_str().unwrap()
    );
}

// Mirrors tests/test_db.py.

#[test]
fn init_creates_all_tables_and_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("nested/dir/test.db");
    let names = Store::open(&db).unwrap().table_names().unwrap();
    for table in TABLES {
        assert!(names.iter().any(|n| n == table), "missing {table}");
    }
    let again = Store::open(&db).unwrap();
    assert_eq!(again.table_names().unwrap(), names);
    assert!(again.user_version().unwrap() >= 2);
}

#[test]
fn count_rows_rejects_unknown_tables() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("t.db")).unwrap();
    assert_eq!(
        store
            .count_rows("sqlite_master; DROP TABLE sessions")
            .unwrap_err()
            .to_string(),
        "unknown table: sqlite_master; DROP TABLE sessions"
    );
}

#[cfg(unix)]
#[test]
fn new_database_is_private() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("private.db");
    Store::open(&db).unwrap();
    let mode = std::fs::metadata(&db).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
}
