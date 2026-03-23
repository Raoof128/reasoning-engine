from reasoning_engine.db import get_connection
from reasoning_engine.memory import recall_memory, record_reflection, save_memory


def _create_session(db_path, session_id="sess-1", query="test query"):
    """Helper to insert a session row so FK constraints are satisfied."""
    with get_connection(db_path) as conn:
        conn.execute(
            "INSERT INTO sessions (id, query) VALUES (?, ?)",
            (session_id, query),
        )


def _create_branch(db_path, branch_id="branch-1", session_id="sess-1"):
    """Helper to insert a branch row so FK constraints are satisfied."""
    with get_connection(db_path) as conn:
        conn.execute(
            "INSERT INTO branches (id, session_id) VALUES (?, ?)",
            (branch_id, session_id),
        )


def test_save_and_recall_memory(db_path):
    _create_session(db_path, session_id="sess-1", query="How do PRMs work?")
    save_memory(
        db_path,
        session_id="sess-1",
        query="How do PRMs work?",
        key_learnings=["PRMs score individual steps", "Dense supervision beats ORM"],
        domain_tags=["prm", "reward-models"],
    )
    results = recall_memory(db_path, query="process reward models")
    assert len(results) >= 1
    assert "PRMs score individual steps" in results[0]["key_learnings"]


def test_recall_empty(db_path):
    results = recall_memory(db_path, query="completely unrelated query xyz")
    assert results == []


def test_record_and_retrieve_reflection(db_path):
    _create_session(db_path, session_id="sess-1")
    _create_branch(db_path, branch_id="branch-1", session_id="sess-1")
    reflection = record_reflection(
        db_path,
        branch_id="branch-1",
        session_id="sess-1",
        original_critique="Missing citations for claim X",
        revision_summary="Added 3 sources verifying claim X",
        score_before=0.4,
        score_after=0.7,
    )
    assert reflection["id"]
    assert reflection["score_after"] > reflection["score_before"]
