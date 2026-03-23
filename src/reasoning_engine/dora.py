"""DORA — Direction-Oriented Resource Allocation."""

from dataclasses import dataclass

KAPPA_THRESHOLD = 0.15
REFLEXION_THRESHOLD = 0.5
PRUNE_THRESHOLD = 0.25

DIFFICULTY_LOW = 0.3
DIFFICULTY_MEDIUM = 0.5
DIFFICULTY_HIGH = 0.7
DEPTH_MAX_BRANCHES = 2


@dataclass
class BudgetAllocation:
    strategy: str
    total_branches: int
    max_depth: int
    max_steps: int
    tokens_per_branch: int
    reflexion_rounds: int


@dataclass
class AllocationResult:
    branches_to_continue: list[str]
    branches_to_reflect: list[str]
    branches_to_prune: list[str]
    allocation: str
    kappa: float
    budget_remaining: int


def allocate_budget(difficulty: float) -> BudgetAllocation:
    if difficulty < DIFFICULTY_LOW:
        return BudgetAllocation("single_pass", 1, 3, 5, 2000, 0)
    elif difficulty < DIFFICULTY_MEDIUM:
        return BudgetAllocation("best_of_n", 3, 5, 15, 3000, 1)
    elif difficulty < DIFFICULTY_HIGH:
        return BudgetAllocation("beam_search", 5, 8, 30, 4000, 2)
    else:
        return BudgetAllocation("forest", 8, 12, 50, 5000, 3)


def select_branches(scores: dict[str, float], budget_remaining: int) -> AllocationResult:
    if not scores:
        return AllocationResult([], [], [], "breadth", 0.0, budget_remaining)

    score_values = list(scores.values())
    kappa = (max(score_values) - min(score_values)) if len(score_values) > 1 else 0.0
    sorted_branches = sorted(scores.items(), key=lambda x: x[1], reverse=True)

    to_continue, to_reflect, to_prune = [], [], []

    if kappa > KAPPA_THRESHOLD:
        allocation = "breadth"
        for branch_id, score in sorted_branches:
            if score < PRUNE_THRESHOLD:
                to_prune.append(branch_id)
            elif score < REFLEXION_THRESHOLD:
                to_reflect.append(branch_id)
            else:
                to_continue.append(branch_id)
    else:
        allocation = "depth"
        top_n = max(1, min(DEPTH_MAX_BRANCHES, len(sorted_branches)))
        for i, (branch_id, score) in enumerate(sorted_branches):
            if i < top_n:
                to_continue.append(branch_id)
            elif score < REFLEXION_THRESHOLD:
                to_reflect.append(branch_id)
            else:
                to_prune.append(branch_id)

    steps_used = len(to_continue) + len(to_reflect)
    return AllocationResult(
        to_continue, to_reflect, to_prune, allocation, kappa, budget_remaining - steps_used
    )
