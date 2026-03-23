from reasoning_engine.dora import allocate_budget, select_branches


def test_easy_query_gets_single_pass():
    budget = allocate_budget(difficulty=0.1)
    assert budget.strategy == "single_pass"
    assert budget.total_branches == 1
    assert budget.max_depth <= 4
    assert budget.reflexion_rounds == 0


def test_medium_query_gets_best_of_n():
    budget = allocate_budget(difficulty=0.4)
    assert budget.strategy == "best_of_n"
    assert budget.total_branches == 3


def test_hard_query_gets_beam_search():
    budget = allocate_budget(difficulty=0.6)
    assert budget.strategy == "beam_search"
    assert budget.total_branches == 5


def test_very_hard_query_gets_forest():
    budget = allocate_budget(difficulty=0.85)
    assert budget.strategy == "forest"
    assert budget.total_branches == 8
    assert budget.reflexion_rounds == 3


def test_select_branches_high_kappa_explores():
    scores = {"b1": 0.9, "b2": 0.2, "b3": 0.5}
    result = select_branches(scores, budget_remaining=10)
    assert result.allocation == "breadth"
    assert len(result.branches_to_continue) >= 2
    assert result.kappa > 0.15


def test_select_branches_low_kappa_exploits():
    scores = {"b1": 0.81, "b2": 0.79, "b3": 0.80}
    result = select_branches(scores, budget_remaining=10)
    assert result.allocation == "depth"
    assert len(result.branches_to_continue) <= 2


def test_select_branches_flags_low_scorers_for_reflexion():
    scores = {"b1": 0.9, "b2": 0.3, "b3": 0.7}
    result = select_branches(scores, budget_remaining=10)
    assert "b2" in result.branches_to_reflect


def test_select_branches_prunes_worst():
    scores = {"b1": 0.9, "b2": 0.1, "b3": 0.7, "b4": 0.8}
    result = select_branches(scores, budget_remaining=5)
    assert "b2" in result.branches_to_prune
