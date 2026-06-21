import json
import os
import tempfile

import pytest

# Set temp DB before importing server.
_tmp_db = tempfile.NamedTemporaryFile(suffix=".db", delete=False)  # noqa: SIM115
_tmp_db.close()
os.environ["REASONING_ENGINE_DB"] = _tmp_db.name

from reasoning_engine.server import (  # noqa: E402
    check_termination,
    get_session_state,
    init_research_session,
    plan_research_angles_tool,
    register_branch,
    sanitize_content,
    score_branch,
    select_next_branches,
)


def test_init_research_session():
    data = json.loads(init_research_session("How do process reward models work?"))
    assert data["session_id"]
    assert data["difficulty"] >= 0.0
    assert data["strategy"]
    assert data["budget"]


def test_full_workflow():
    session = json.loads(init_research_session("Simple test query"))
    sid = session["session_id"]

    branch = json.loads(
        register_branch(
            sid,
            trace=json.dumps(["Step 1: researched X"]),
            sources=json.dumps([{"url": "https://example.com", "title": "Ex"}]),
        )
    )

    score_branch(
        sid,
        branch["branch_id"],
        q_score=0.9,
        advantage=0.4,
        critique="Strong analysis",
        confidence=0.85,
    )

    term = json.loads(check_termination(sid))
    assert term["should_terminate"] is True


def test_sanitize_content():
    cleaned = json.loads(sanitize_content("Hello <script>bad</script> world"))
    assert "<script>" not in cleaned["cleaned"]
    assert "world" in cleaned["cleaned"]


def test_select_next_branches():
    session = json.loads(init_research_session("Compare beam search and MCTS for reasoning tasks"))
    sid = session["session_id"]

    for trace in [["Path A"], ["Path B"], ["Path C"]]:
        branch = json.loads(register_branch(sid, trace=json.dumps(trace)))
        score_branch(
            sid,
            branch["branch_id"],
            q_score=0.5,
            advantage=0.2,
            critique="OK",
            confidence=0.6,
        )

    data = json.loads(select_next_branches(sid))
    assert "branches_to_continue" in data
    assert "kappa" in data


def test_score_branch_rejects_invalid_score():
    session = json.loads(init_research_session("Simple test query"))
    branch = json.loads(register_branch(session["session_id"], trace=json.dumps(["Step 1"])))

    with pytest.raises(ValueError, match="q_score"):
        score_branch(
            session["session_id"],
            branch["branch_id"],
            q_score=1.5,
            advantage=0.2,
            critique="Invalid",
            confidence=0.6,
        )


def test_planning_persists_budget():
    session = json.loads(init_research_session("Simple"))
    sid = session["session_id"]
    branch = json.loads(register_branch(sid, trace=json.dumps(["Step 1"])))
    score_branch(
        sid,
        branch["branch_id"],
        q_score=0.5,
        advantage=0.2,
        critique="OK",
        confidence=0.6,
    )

    for _ in range(session["budget"]["max_steps"]):
        select_next_branches(sid)

    state = json.loads(get_session_state(sid))
    assert state["session"]["budget_remaining_steps"] == 0


def test_plan_research_angles_tool():
    angles = json.loads(
        plan_research_angles_tool("Compare MCP security evaluation methods", max_angles=3)
    )
    assert len(angles) == 3
    assert "name" in angles[0]
