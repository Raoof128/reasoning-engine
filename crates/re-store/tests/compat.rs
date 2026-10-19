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
    assert_eq!(store.user_version().unwrap(), 4);
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

/// `evidence_records` as Python and Rust created it before version 4: keyed
/// by the bare per-run evidence ID.
const V3_EVIDENCE_TABLE: &str = "
CREATE TABLE evidence_records (
    evidence_id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES research_runs(run_id),
    payload TEXT NOT NULL,
    snippet_hash TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
";

/// The claim tables as Python and Rust created them before version 3: keyed
/// by the bare per-run claim and verification IDs.
const V2_CLAIM_TABLES: &str = "
CREATE TABLE claims (
    claim_id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES research_runs(run_id),
    payload TEXT NOT NULL,
    status TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE TABLE claim_verifications (
    verification_id TEXT PRIMARY KEY,
    claim_id TEXT NOT NULL REFERENCES claims(claim_id),
    run_id TEXT NOT NULL REFERENCES research_runs(run_id),
    payload TEXT NOT NULL,
    support_status TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
";

/// Write an older database whose `legacy_tables` predate the current
/// schema, holding two runs where `run_b`'s `ev_0001` and `claim_0001` already
/// overwrote `run_a`'s.
fn write_legacy_db(db: &std::path::Path, legacy_tables: &str, version: i64) {
    let conn = rusqlite::Connection::open(db).unwrap();
    // ver_a1's claim is gone; a version-3 file migrated from version 2
    // carries such rows too.
    conn.pragma_update(None, "foreign_keys", "OFF").unwrap();
    conn.execute_batch(legacy_tables).unwrap();
    conn.execute_batch(re_store::schema::SCHEMA_SQL).unwrap();
    conn.pragma_update(None, "user_version", version).unwrap();
    conn.execute_batch(
        r#"
        INSERT INTO research_runs VALUES
            ('run_a', 'A', 'general', 'standard', 'complete', '2026-01-01T00:00:00Z'),
            ('run_b', 'B', 'general', 'standard', 'complete', '2026-01-02T00:00:00Z');
        INSERT INTO evidence_records VALUES
            ('ev_0001', 'run_b', '{"snippet": "B1"}', 'sha256:b1', '2026-01-02 00:00:00'),
            ('ev_0002', 'run_a', '{"snippet": "A2"}', 'sha256:a2', '2026-01-01 00:00:00');
        INSERT INTO claims VALUES
            ('claim_0001', 'run_b', '{"text": "B1"}', 'supported', '2026-01-02 00:00:00'),
            ('claim_0002', 'run_a', '{"text": "A2"}', 'supported', '2026-01-01 00:00:00');
        INSERT INTO claim_verifications VALUES
            ('ver_a1', 'claim_0001', 'run_a', '{"v": "a1"}', 'supported', '2026-01-01 00:00:00'),
            ('ver_a2', 'claim_0002', 'run_a', '{"v": "a2"}', 'supported', '2026-01-01 00:00:00'),
            ('ver_b1', 'claim_0001', 'run_b', '{"v": "b1"}', 'supported', '2026-01-02 00:00:00');
        "#,
    )
    .unwrap();
}

/// Open the database written by [`write_legacy_db`] and check the migration
/// re-keyed every table by run and kept every row.
fn assert_migrates(legacy_tables: &str, version: i64) {
    use re_model::records::ClaimRecord;
    use serde_json::json;

    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("legacy.db");
    let fresh = dir.path().join("fresh.db");
    write_legacy_db(&db, legacy_tables, version);
    Store::open(&fresh).unwrap();

    let store = Store::open(&db).unwrap();
    assert_eq!(schema_of(&db), schema_of(&fresh));
    assert_eq!(store.user_version().unwrap(), 4);
    assert_eq!(
        store.list_evidence("run_a").unwrap(),
        [json!({"snippet": "A2"})]
    );
    assert_eq!(
        store.list_evidence("run_b").unwrap(),
        [json!({"snippet": "B1"})]
    );
    assert_eq!(store.list_claims("run_a").unwrap(), [json!({"text": "A2"})]);
    assert_eq!(store.list_claims("run_b").unwrap(), [json!({"text": "B1"})]);
    assert_eq!(store.list_verifications("run_a").unwrap().len(), 2);
    assert_eq!(store.list_verifications("run_b").unwrap().len(), 1);
    let conn = rusqlite::Connection::open(&db).unwrap();
    let evidence_row: (String, String) = conn
        .query_row(
            "SELECT snippet_hash, created_at FROM evidence_records WHERE run_id = 'run_a'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        evidence_row,
        ("sha256:a2".into(), "2026-01-01 00:00:00".into())
    );
    let claim_created: String = conn
        .query_row(
            "SELECT created_at FROM claims WHERE run_id = 'run_a'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(claim_created, "2026-01-01 00:00:00");

    // From now on run_a can store its claim_0001 beside run_b's.
    let claim = ClaimRecord::new(
        "claim_0001",
        "run_a",
        "A1",
        "empirical",
        "general",
        "high",
        "low",
        true,
        "test",
    )
    .unwrap();
    store.save_claim(&claim).unwrap();
    assert_eq!(store.list_claims("run_a").unwrap().len(), 2);
    assert_eq!(store.list_claims("run_b").unwrap(), [json!({"text": "B1"})]);
    drop(store);

    // Reopening is a no-op.
    let schema = schema_of(&db);
    Store::open(&db).unwrap();
    assert_eq!(schema_of(&db), schema);
}

#[test]
fn migrates_version_2_tables_keeping_every_row() {
    assert_migrates(&format!("{V3_EVIDENCE_TABLE}{V2_CLAIM_TABLES}"), 2);
}

#[test]
fn migrates_version_3_evidence_table_keeping_every_row() {
    assert_migrates(V3_EVIDENCE_TABLE, 3);
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

#[test]
fn research_ledger_bytes_and_hashes_match_python() {
    use re_core::hashing::stable_json_hash;
    use re_model::records::{
        ClaimRecord, EvidenceGap, EvidenceRecord, ResearchRun, VerificationRecord,
    };

    let dir = tempfile::tempdir().unwrap();
    let manifest = make_python_db(&dir.path().join("python.db"));
    let rows = &manifest["rows"];
    let store = Store::open(dir.path().join("rust.db")).unwrap();

    let parse = |text: &serde_json::Value| -> serde_json::Value {
        serde_json::from_str(text.as_str().unwrap()).unwrap()
    };
    let run: ResearchRun = serde_json::from_value(rows["research_runs"][0].clone()).unwrap();
    store.save_run(&run).unwrap();
    assert_eq!(
        serde_json::to_value(store.get_run(&run.run_id).unwrap()).unwrap(),
        rows["research_runs"][0]
    );

    // Re-save Python's records from their payloads; the stored text must be
    // byte-identical, including escaped non-ASCII and the 1e-05 float.
    let evidence: EvidenceRecord =
        serde_json::from_value(parse(&rows["evidence_records"][0]["payload"])).unwrap();
    store.save_evidence(&evidence).unwrap();
    for row in rows["claims"].as_array().unwrap() {
        let claim: ClaimRecord = serde_json::from_value(parse(&row["payload"])).unwrap();
        store.save_claim(&claim).unwrap();
    }
    for row in rows["claim_verifications"].as_array().unwrap() {
        let v: VerificationRecord = serde_json::from_value(parse(&row["payload"])).unwrap();
        store.save_verification(&run.run_id, &v).unwrap();
    }
    let gap: EvidenceGap =
        serde_json::from_value(parse(&rows["evidence_gaps"][0]["payload"])).unwrap();
    store.save_gap(&gap).unwrap();

    let conn = rusqlite::Connection::open(store.path()).unwrap();
    for (table, key) in [
        ("evidence_records", "evidence_id"),
        ("claims", "claim_id"),
        ("claim_verifications", "verification_id"),
        ("evidence_gaps", "gap_id"),
    ] {
        for row in rows[table].as_array().unwrap() {
            let id = row[key].as_str().unwrap();
            let ours: String = conn
                .query_row(
                    &format!("SELECT payload FROM {table} WHERE {key} = ?"),
                    [id],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(ours, row["payload"].as_str().unwrap(), "{table} {id}");
        }
    }
    let ours_hash: String = conn
        .query_row("SELECT snippet_hash FROM evidence_records", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(
        ours_hash,
        rows["evidence_records"][0]["snippet_hash"]
            .as_str()
            .unwrap()
    );

    // Python's provenance IDs are stable_json_hash(event) + ":" + random hex.
    for row in rows["provenance_events"].as_array().unwrap() {
        let (hash, _suffix) = row["event_id"].as_str().unwrap().rsplit_once(':').unwrap();
        assert_eq!(stable_json_hash(&parse(&row["payload"])), hash);
    }
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

#[test]
fn fts_and_legacy_recall_agree_on_the_python_fixture() {
    let dir = tempfile::tempdir().expect("tmp");
    let db = dir.path().join("python.db");
    make_python_db(&db);
    let before = schema_of(&db);
    let store = re_store::Store::open(&db).expect("open");
    for query in ["How do PRMs work", "reward models", "prm"] {
        let legacy = store.recall_memory(query, 5).expect("legacy");
        let fts = store.recall_memory_fts(query, 5).expect("fts");
        assert_eq!(
            fts.first().map(|m| &m.id),
            legacy.first().map(|m| &m.id),
            "{query}"
        );
    }
    drop(store);
    assert_eq!(
        schema_of(&db),
        before,
        "FTS5 index lives in temp, never in the file"
    );
}
