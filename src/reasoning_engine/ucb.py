"""Upper Confidence Bound (UCB1) selection for reasoning branches."""

import math

DEFAULT_C = 1.4


def ucb_score(q_value: float, parent_visits: int, node_visits: int, c: float = DEFAULT_C) -> float:
    if node_visits == 0:
        return float("inf")
    return q_value + c * math.sqrt(math.log(parent_visits) / node_visits)


def select_best_ucb(branches: dict[str, dict], total_visits: int, c: float = DEFAULT_C) -> str | None:
    if not branches:
        return None
    best_id = None
    best_score = -float("inf")
    for branch_id, data in branches.items():
        score = ucb_score(data["q_score"], max(total_visits, 1), data["visits"], c)
        if score > best_score:
            best_score = score
            best_id = branch_id
    return best_id
