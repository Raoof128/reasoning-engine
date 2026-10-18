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
