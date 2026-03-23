from reasoning_engine.difficulty import estimate_difficulty


def test_simple_query_low_difficulty():
    score = estimate_difficulty("What is photosynthesis?")
    assert 0.0 <= score <= 0.3


def test_complex_query_high_difficulty():
    score = estimate_difficulty(
        "How do process reward models interact with Monte Carlo tree search "
        "in the context of test-time compute scaling for reasoning-focused LLMs, "
        "and what are the implications for agentic task execution across "
        "heterogeneous tool environments?"
    )
    assert score >= 0.5


def test_medium_query_medium_difficulty():
    score = estimate_difficulty(
        "Compare the effectiveness of beam search vs best-of-N sampling "
        "for mathematical reasoning tasks"
    )
    assert 0.2 <= score <= 0.8


def test_difficulty_always_in_bounds():
    for query in ["hi", "x" * 5000, "", "a b c"]:
        score = estimate_difficulty(query)
        assert 0.0 <= score <= 1.0
