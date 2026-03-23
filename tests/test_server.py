import json
import os
import tempfile

import pytest

# Set temp DB before importing server
_tmp_db = tempfile.NamedTemporaryFile(suffix=".db", delete=False)  # noqa: SIM115
_tmp_db.close()
os.environ["REASONING_ENGINE_DB"] = _tmp_db.name

from fastmcp import Client  # noqa: E402

from reasoning_engine.server import mcp  # noqa: E402


def _text(result):
    """Extract text from a CallToolResult."""
    return result.content[0].text


@pytest.fixture
def client():
    return Client(mcp)


@pytest.mark.asyncio
async def test_init_research_session(client):
    async with client:
        result = await client.call_tool(
            "init_research_session", {"query": "How do process reward models work?"}
        )
        data = json.loads(_text(result))
        assert data["session_id"]
        assert data["difficulty"] >= 0.0
        assert data["strategy"]
        assert data["budget"]


@pytest.mark.asyncio
async def test_full_workflow(client):
    async with client:
        # Init
        init_result = await client.call_tool(
            "init_research_session", {"query": "Simple test query"}
        )
        session = json.loads(_text(init_result))
        sid = session["session_id"]

        # Register branch
        branch_result = await client.call_tool(
            "register_branch",
            {
                "session_id": sid,
                "trace": json.dumps(["Step 1: researched X"]),
                "sources": json.dumps([{"url": "https://example.com", "title": "Ex"}]),
            },
        )
        branch = json.loads(_text(branch_result))

        # Score branch
        await client.call_tool(
            "score_branch",
            {
                "session_id": sid,
                "branch_id": branch["branch_id"],
                "q_score": 0.9,
                "advantage": 0.4,
                "critique": "Strong analysis",
                "confidence": 0.85,
            },
        )

        # Check termination
        term_result = await client.call_tool("check_termination", {"session_id": sid})
        term = json.loads(_text(term_result))
        assert term["should_terminate"] is True


@pytest.mark.asyncio
async def test_sanitize_content(client):
    async with client:
        result = await client.call_tool(
            "sanitize_content", {"raw_text": "Hello <script>bad</script> world"}
        )
        cleaned = json.loads(_text(result))
        assert "<script>" not in cleaned["cleaned"]
        assert "world" in cleaned["cleaned"]


@pytest.mark.asyncio
async def test_select_next_branches(client):
    async with client:
        init = await client.call_tool(
            "init_research_session", {"query": "Compare beam search and MCTS for reasoning tasks"}
        )
        session = json.loads(_text(init))
        sid = session["session_id"]

        for trace in [["Path A"], ["Path B"], ["Path C"]]:
            br = await client.call_tool(
                "register_branch", {"session_id": sid, "trace": json.dumps(trace)}
            )
            branch = json.loads(_text(br))
            await client.call_tool(
                "score_branch",
                {
                    "session_id": sid,
                    "branch_id": branch["branch_id"],
                    "q_score": 0.5,
                    "advantage": 0.2,
                    "critique": "OK",
                    "confidence": 0.6,
                },
            )

        result = await client.call_tool("select_next_branches", {"session_id": sid})
        data = json.loads(_text(result))
        assert "branches_to_continue" in data
        assert "kappa" in data
