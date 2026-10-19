//! Read-only listings behind the audit UI.

use re_model::records::ResearchRun;
use re_store::Store;

fn store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("test.db")).unwrap();
    (dir, store)
}

#[test]
fn sessions_and_runs_list_newest_first_within_the_limit() {
    let (_dir, store) = store();
    let ids: Vec<String> = ["first", "second", "third"]
        .iter()
        .map(|q| store.create_session(q).unwrap().id)
        .collect();
    let listed: Vec<String> = store
        .list_sessions(2)
        .unwrap()
        .into_iter()
        .map(|s| s.id)
        .collect();
    // Same-second rows fall back to insertion order, newest first.
    assert_eq!(listed, [ids[2].clone(), ids[1].clone()]);
    assert_eq!(store.list_sessions(0).unwrap(), []);

    for query in ["a", "b"] {
        store
            .save_run(&ResearchRun::create(query, "general", "standard"))
            .unwrap();
    }
    let runs = store.list_runs(10).unwrap();
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].query, "b");
}

#[test]
fn reflections_list_oldest_first_for_one_session() {
    let (_dir, store) = store();
    let session = store.create_session("reflect").unwrap().id;
    let other = store.create_session("other").unwrap().id;
    let branch = store
        .register_branch(&session, &["step".into()], &[], None, 0)
        .unwrap()
        .id;
    let elsewhere = store
        .register_branch(&other, &["step".into()], &[], None, 0)
        .unwrap()
        .id;
    store
        .record_reflection(&branch, &session, "weak", "stronger", 0.3, 0.6)
        .unwrap();
    store
        .record_reflection(&branch, &session, "still weak", "better", 0.6, 0.8)
        .unwrap();
    store
        .record_reflection(&elsewhere, &other, "x", "y", 0.1, 0.2)
        .unwrap();
    let reflections = store.list_reflections(&session).unwrap();
    assert_eq!(reflections.len(), 2);
    assert_eq!(reflections[0].original_critique, "weak");
    assert!((reflections[1].score_after - 0.8).abs() < 1e-12);
    assert_eq!(store.list_reflections("missing").unwrap(), []);
}
