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
    classify_research_mode_tool,
    export_run_pack_tool,
    get_scholar_auth_status,
    get_session_state,
    init_research_session,
    plan_research_angles_tool,
    register_branch,
    run_quality_gate_tool,
    run_research_pipeline_tool,
    sanitize_content,
    score_branch,
    select_next_branches,
    scholar_search_tool,
    start_research_run,
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


def test_verifiable_start_research_run_tool():
    data = json.loads(start_research_run("Explain MCP prompt injection", mode="standard", profile="auto"))

    assert data["run_id"].startswith("run_")
    assert data["profile"] == "security"
    assert data["mode"] == "high_stakes"


def test_classify_research_mode_tool():
    data = json.loads(classify_research_mode_tool("Can this vulnerability leak credentials?"))

    assert data["mode"] == "high_stakes"


def test_verifiable_scholar_search_tool_uses_mock_by_default():
    run = json.loads(start_research_run("Scholar Gateway semantic search"))
    data = json.loads(scholar_search_tool(run["run_id"], "Scholar Gateway semantic search", limit=1))

    assert data["evidence"][0]["source_adapter"] == "scholar_gateway"
    assert data["error"] is None


def test_get_scholar_auth_status_does_not_expose_token(monkeypatch):
    monkeypatch.setenv("SCHOLAR_GATEWAY_ACCESS_TOKEN", "secret-token")
    data = json.loads(get_scholar_auth_status())

    assert data["has_env_token"] is True
    assert "secret-token" not in json.dumps(data)


def test_run_research_pipeline_tool_exports_pack():
    data = json.loads(
        run_research_pipeline_tool(
            "Scholar Gateway exposes semantic search",
            "Scholar Gateway exposes semantic search.",
            mode="standard",
            profile="general",
        )
    )

    assert data["run_id"].startswith("run_")
    assert data["attestation"]["valid"] is True


def test_run_quality_gate_tool_reads_persisted_claims():
    data = json.loads(
        run_research_pipeline_tool(
            "Scholar Gateway exposes semantic search",
            "Scholar Gateway exposes semantic search.",
            mode="standard",
            profile="general",
        )
    )

    gate = json.loads(run_quality_gate_tool(data["run_id"]))

    assert gate["result"] == "pass"


def test_export_run_pack_tool_aliases_pipeline():
    data = json.loads(
        export_run_pack_tool(
            "Scholar Gateway exposes semantic search",
            "Scholar Gateway exposes semantic search.",
            mode="standard",
            profile="general",
        )
    )

    assert data["attestation"]["valid"] is True
