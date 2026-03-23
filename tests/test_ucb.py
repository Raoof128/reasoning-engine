import math

from reasoning_engine.ucb import select_best_ucb, ucb_score


def test_ucb_score_basic():
    score = ucb_score(q_value=0.5, parent_visits=10, node_visits=2, c=1.4)
    expected = 0.5 + 1.4 * math.sqrt(math.log(10) / 2)
    assert abs(score - expected) < 1e-6


def test_ucb_unvisited_node_gets_infinity():
    score = ucb_score(q_value=0.0, parent_visits=10, node_visits=0, c=1.4)
    assert score == float("inf")


def test_select_best_ucb_picks_highest():
    branches = {
        "b1": {"q_score": 0.8, "visits": 5},
        "b2": {"q_score": 0.6, "visits": 1},
        "b3": {"q_score": 0.7, "visits": 3},
    }
    total_visits = 9
    best = select_best_ucb(branches, total_visits)
    assert best == "b2"


def test_select_best_ucb_prefers_unvisited():
    branches = {
        "b1": {"q_score": 0.9, "visits": 10},
        "b2": {"q_score": 0.0, "visits": 0},
    }
    best = select_best_ucb(branches, total_visits=10)
    assert best == "b2"
