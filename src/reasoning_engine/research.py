"""Research-planning helpers for the MCP server."""

from __future__ import annotations

import re
from dataclasses import asdict, dataclass

from reasoning_engine.difficulty import estimate_difficulty
from reasoning_engine.validation import MAX_QUESTION_CHARS, unique_ordered, validate_text

ANGLE_LIBRARY = [
    ("conceptual foundations", "Define core concepts, assumptions, and terminology."),
    ("empirical evidence", "Find measurements, benchmarks, replications, and negative results."),
    ("mechanisms", "Explain causal mechanisms, algorithms, or system internals."),
    ("comparative analysis", "Compare alternatives, baselines, and trade-offs."),
    ("limitations and failure modes", "Identify boundary conditions, risks, and known failures."),
    ("implementation practice", "Translate findings into design or operational guidance."),
    ("recent developments", "Look for current work, releases, and active debates."),
    ("contrarian perspective", "Test whether the dominant claim is overstated or incomplete."),
    ("security and abuse cases", "Review misuse, adversarial, privacy, and safety angles."),
    ("evaluation methodology", "Define metrics, datasets, and validation criteria."),
]

DOMAIN_HINTS = {
    "security": "security and abuse cases",
    "mcp": "security and abuse cases",
    "benchmark": "evaluation methodology",
    "eval": "evaluation methodology",
    "evaluation": "evaluation methodology",
    "compare": "comparative analysis",
    "vs": "comparative analysis",
    "recent": "recent developments",
    "latest": "recent developments",
    "implementation": "implementation practice",
    "deploy": "implementation practice",
}

QUESTION_STARTERS = (
    "What is the strongest evidence for this claim?",
    "What would change the conclusion?",
    "Which sources are primary, current, and independently corroborated?",
    "Which assumptions are unresolved?",
)


@dataclass(frozen=True)
class ResearchAngle:
    name: str
    objective: str
    priority: int
    starter_questions: list[str]


def plan_research_angles(query: str, max_angles: int = 6) -> list[dict]:
    query = validate_text(query, "query", MAX_QUESTION_CHARS)
    words = set(re.findall(r"[a-z0-9-]+", query.lower()))
    selected_names = []
    for word in words:
        if word in DOMAIN_HINTS:
            selected_names.append(DOMAIN_HINTS[word])

    difficulty = estimate_difficulty(query)
    if difficulty >= 0.5:
        selected_names.extend(
            [
                "conceptual foundations",
                "empirical evidence",
                "comparative analysis",
                "limitations and failure modes",
                "evaluation methodology",
            ]
        )
    else:
        selected_names.extend(
            ["conceptual foundations", "empirical evidence", "implementation practice"]
        )

    selected_names.extend(name for name, _objective in ANGLE_LIBRARY)
    selected_names = unique_ordered(selected_names)[:max_angles]
    objective_by_name = dict(ANGLE_LIBRARY)

    return [
        asdict(
            ResearchAngle(
                name=name,
                objective=objective_by_name[name],
                priority=index + 1,
                starter_questions=[
                    QUESTION_STARTERS[index % len(QUESTION_STARTERS)],
                    f"What does the {name} angle add that other paths might miss?",
                ],
            )
        )
        for index, name in enumerate(selected_names)
    ]


def evidence_gap_questions(query: str, claims: list[str]) -> list[dict]:
    query = validate_text(query, "query", MAX_QUESTION_CHARS)
    gaps = []
    for index, claim in enumerate(claims, start=1):
        normalized = validate_text(claim, f"claims[{index - 1}]", MAX_QUESTION_CHARS)
        gaps.append(
            {
                "claim_id": index,
                "claim": normalized,
                "questions": [
                    f"What primary source directly supports claim {index} for this query?",
                    "Is there a newer source or replication that contradicts it?",
                    "What citation would let a reader independently verify the claim?",
                    "Does the source prove the exact claim, or only a weaker related point?",
                ],
                "minimum_evidence": "At least one primary source plus one independent corroborating source for important claims.",
                "query_context": query,
            }
        )
    return gaps
