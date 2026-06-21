from reasoning_engine.research import evidence_gap_questions, plan_research_angles


def test_plan_research_angles_prioritizes_relevant_angles():
    angles = plan_research_angles("Compare MCP security evaluation methods", max_angles=4)
    names = [angle["name"] for angle in angles]
    assert "security and abuse cases" in names
    assert "evaluation methodology" in names


def test_evidence_gap_questions_returns_claim_checks():
    gaps = evidence_gap_questions(
        "What improves RAG factuality?", ["Self-RAG improves citation accuracy"]
    )
    assert gaps[0]["claim_id"] == 1
    assert "primary source" in gaps[0]["questions"][0]
