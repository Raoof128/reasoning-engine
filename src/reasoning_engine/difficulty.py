"""Heuristic difficulty estimator for research queries."""

import re

COMPLEX_KEYWORDS = {
    "implications",
    "interaction",
    "heterogeneous",
    "integration",
    "trade-offs",
    "tradeoffs",
    "compare",
    "contrast",
    "analyze",
    "synthesize",
    "relationship",
    "architecture",
    "framework",
    "mechanism",
    "algorithm",
    "optimization",
    "scaling",
    "cross-domain",
    "multi-step",
    "multi-modal",
    "emergent",
}

DOMAIN_KEYWORDS = {
    "llm",
    "transformer",
    "attention",
    "reinforcement",
    "reward",
    "neural",
    "gradient",
    "backpropagation",
    "embedding",
    "tokenization",
    "mcts",
    "beam search",
    "tree search",
    "chain-of-thought",
    "process reward",
    "reflexion",
    "self-critique",
    "agentic",
    "prm",
    "orm",
    "rlhf",
    "dpo",
    "ppo",
    "grpo",
}

MULTI_PART_SIGNALS = re.compile(
    r"\b(and|or|but|however|moreover|furthermore|additionally|"
    r"versus|vs\.?|compared to|in relation to|"
    r"how do .+ interact with|what are the implications)\b",
    re.IGNORECASE,
)


def estimate_difficulty(query: str) -> float:
    if not query.strip():
        return 0.0
    words = query.lower().split()
    word_count = len(words)
    word_set = set(words)
    length_score = min(word_count / 60.0, 1.0)
    query_lower = query.lower()
    complex_hits = len(word_set & COMPLEX_KEYWORDS)
    # Check single-word domain keywords via set intersection, and
    # multi-word domain phrases via substring search on the full query.
    single_word_domain = {kw for kw in DOMAIN_KEYWORDS if " " not in kw}
    multi_word_domain = {kw for kw in DOMAIN_KEYWORDS if " " in kw}
    domain_hits = len(word_set & single_word_domain)
    domain_hits += sum(1 for phrase in multi_word_domain if phrase in query_lower)
    keyword_score = min((complex_hits + domain_hits * 1.5) / 8.0, 1.0)
    multi_part_matches = len(MULTI_PART_SIGNALS.findall(query))
    structure_score = min(multi_part_matches / 3.0, 1.0)
    question_count = query.count("?")
    question_score = min(question_count / 3.0, 1.0)
    # Difficulty scoring weights:
    #   0.25 — length_score:    longer queries tend to be more complex
    #   0.35 — keyword_score:   presence of technical/complex keywords (highest weight)
    #   0.25 — structure_score: multi-part conjunctions signal multi-faceted questions
    #   0.15 — question_score:  multiple question marks indicate compound questions
    difficulty = (
        0.25 * length_score + 0.35 * keyword_score + 0.25 * structure_score + 0.15 * question_score
    )
    return max(0.0, min(1.0, difficulty))
