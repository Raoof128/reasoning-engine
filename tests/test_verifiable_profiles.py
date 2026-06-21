from reasoning_engine.verifiable.profiles import (
    classify_research_mode,
    get_profile,
    list_profiles,
    select_profile,
)


def test_list_profiles_includes_required_profiles():
    names = [profile.name for profile in list_profiles()]

    assert "general" in names
    assert "security" in names
    assert "medicine" in names
    assert "ai_safety" in names


def test_select_profile_uses_security_keywords():
    profile = select_profile("How should MCP tools handle prompt injection?")

    assert profile.name == "security"
    assert profile.claim_strictness == "high"


def test_select_profile_defaults_to_general():
    profile = select_profile("Summarize the history of printing")

    assert profile.name == "general"


def test_classify_mode_escalates_high_stakes_security():
    mode = classify_research_mode("Can this vulnerability leak credentials?", requested_mode="standard")

    assert mode == "high_stakes"


def test_requested_scholarly_mode_is_respected_for_low_risk_query():
    mode = classify_research_mode("Compare literature synthesis methods", requested_mode="scholarly")

    assert mode == "scholarly"


def test_unknown_profile_fails():
    try:
        get_profile("missing")
    except ValueError as exc:
        assert "unknown profile" in str(exc)
    else:
        raise AssertionError("unknown profile should fail")
