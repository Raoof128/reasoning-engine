import json

from reasoning_engine.sessions import (
    check_termination,
    create_session,
    get_active_branches,
    get_branch,
    get_consensus_candidates,
    get_session,
    register_branch,
    score_branch,
    update_branch_status,
)


def test_create_session(db_path):
    session = create_session(db_path, query="What is attention?")
    assert session["id"]
    assert session["query"] == "What is attention?"
    assert session["difficulty"] >= 0.0
    assert session["strategy"] in ("single_pass", "best_of_n", "beam_search", "forest")
    assert session["status"] == "active"


def test_get_session(db_path):
    session = create_session(db_path, query="Test query")
    fetched = get_session(db_path, session["id"])
    assert fetched["id"] == session["id"]
    assert fetched["query"] == "Test query"


def test_register_and_get_branch(db_path):
    session = create_session(db_path, query="Test")
    branch = register_branch(
        db_path,
        session_id=session["id"],
        trace=["Step 1: Research X", "Step 2: Found Y"],
        sources=[{"url": "https://example.com", "title": "Example", "excerpt": "Data"}],
    )
    assert branch["id"]
    assert branch["session_id"] == session["id"]
    assert branch["depth"] == 0

    fetched = get_branch(db_path, branch["id"])
    assert json.loads(fetched["trace"]) == ["Step 1: Research X", "Step 2: Found Y"]


def test_score_branch_dual_signal(db_path):
    session = create_session(db_path, query="Test")
    branch = register_branch(db_path, session["id"], trace=["Step 1"])
    score_branch(
        db_path,
        branch_id=branch["id"],
        q_score=0.75,
        advantage=0.3,
        critique="Good but missing citations",
        confidence=0.8,
    )
    updated = get_branch(db_path, branch["id"])
    assert updated["q_score"] == 0.75
    assert updated["advantage"] == 0.3
    assert updated["critique"] == "Good but missing citations"
    assert updated["confidence"] == 0.8
    assert updated["visits"] == 1


def test_get_active_branches(db_path):
    session = create_session(db_path, query="Test")
    b1 = register_branch(db_path, session["id"], trace=["Path A"])
    b2 = register_branch(db_path, session["id"], trace=["Path B"])
    update_branch_status(db_path, b1["id"], "pruned")

    active = get_active_branches(db_path, session["id"])
    assert len(active) == 1
    assert active[0]["id"] == b2["id"]


def test_check_termination_budget_exhausted(db_path):
    session = create_session(db_path, query="Simple question")
    result = check_termination(db_path, session["id"], budget_remaining=0)
    assert result["should_terminate"] is True
    assert "budget" in result["reason"].lower()


def test_check_termination_high_confidence(db_path):
    session = create_session(db_path, query="Test")
    branch = register_branch(db_path, session["id"], trace=["Path"])
    score_branch(
        db_path, branch["id"], q_score=0.9, advantage=0.5, critique="Excellent", confidence=0.85
    )
    result = check_termination(db_path, session["id"], budget_remaining=10)
    assert result["should_terminate"] is True
    assert "confidence" in result["reason"].lower()


def test_get_consensus_candidates(db_path):
    session = create_session(db_path, query="Test")
    b1 = register_branch(db_path, session["id"], trace=["Good path"])
    b2 = register_branch(db_path, session["id"], trace=["Bad path"])
    score_branch(db_path, b1["id"], q_score=0.9, advantage=0.4, critique="Strong", confidence=0.8)
    score_branch(db_path, b2["id"], q_score=0.3, advantage=0.1, critique="Weak", confidence=0.5)

    candidates = get_consensus_candidates(db_path, session["id"], top_k=1)
    assert len(candidates) == 1
    assert candidates[0]["id"] == b1["id"]
