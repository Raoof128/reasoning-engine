"""Domain profiles and deterministic mode selection."""

from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True)
class ResearchProfile:
    name: str
    preferred_source_types: tuple[str, ...]
    weak_source_types: tuple[str, ...]
    minimum_evidence_count: int
    recency_requirement: str
    citation_style: str
    claim_strictness: str
    high_risk_triggers: tuple[str, ...]
    required_caveats: tuple[str, ...]
    scoring_weights: dict[str, float]


BASE_WEIGHTS = {
    "source_quality": 0.25,
    "citation_support": 0.30,
    "recency": 0.15,
    "contradiction_handling": 0.20,
    "provenance_completeness": 0.10,
}

PROFILES = {
    "general": ResearchProfile(
        name="general",
        preferred_source_types=(
            "peer_reviewed_article",
            "standards_document",
            "book",
            "primary_source",
        ),
        weak_source_types=("social_media",),
        minimum_evidence_count=1,
        recency_requirement="normal",
        citation_style="harvard_au",
        claim_strictness="medium",
        high_risk_triggers=(),
        required_caveats=(),
        scoring_weights=BASE_WEIGHTS,
    ),
    "medicine": ResearchProfile(
        name="medicine",
        preferred_source_types=("peer_reviewed_article", "clinical_guideline", "systematic_review"),
        weak_source_types=("blog_post", "social_media"),
        minimum_evidence_count=2,
        recency_requirement="fast",
        citation_style="vancouver",
        claim_strictness="maximum",
        high_risk_triggers=("diagnosis", "treatment", "drug", "dose", "clinical"),
        required_caveats=("Medical findings require professional clinical review.",),
        scoring_weights=BASE_WEIGHTS,
    ),
    "law": ResearchProfile(
        name="law",
        preferred_source_types=(
            "legislation",
            "case_law",
            "regulator_guidance",
            "peer_reviewed_article",
        ),
        weak_source_types=("blog_post", "social_media"),
        minimum_evidence_count=2,
        recency_requirement="fast",
        citation_style="legal",
        claim_strictness="maximum",
        high_risk_triggers=("liability", "compliance", "contract", "statute", "jurisdiction"),
        required_caveats=("Legal findings require jurisdiction-specific professional review.",),
        scoring_weights=BASE_WEIGHTS,
    ),
    "business": ResearchProfile(
        name="business",
        preferred_source_types=("market_report", "financial_filing", "peer_reviewed_article"),
        weak_source_types=("social_media",),
        minimum_evidence_count=2,
        recency_requirement="normal",
        citation_style="harvard_au",
        claim_strictness="high",
        high_risk_triggers=("investment", "valuation", "forecast", "revenue"),
        required_caveats=(
            "Business and financial conclusions depend on changing market conditions.",
        ),
        scoring_weights=BASE_WEIGHTS,
    ),
    "engineering": ResearchProfile(
        name="engineering",
        preferred_source_types=("standards_document", "technical_report", "peer_reviewed_article"),
        weak_source_types=("social_media",),
        minimum_evidence_count=2,
        recency_requirement="normal",
        citation_style="ieee",
        claim_strictness="high",
        high_risk_triggers=("safety", "failure", "load", "certification"),
        required_caveats=("Engineering claims require context-specific validation.",),
        scoring_weights=BASE_WEIGHTS,
    ),
    "science": ResearchProfile(
        name="science",
        preferred_source_types=("peer_reviewed_article", "preprint", "dataset"),
        weak_source_types=("blog_post", "social_media"),
        minimum_evidence_count=2,
        recency_requirement="normal",
        citation_style="harvard_au",
        claim_strictness="high",
        high_risk_triggers=("causal", "replication", "statistically significant"),
        required_caveats=(),
        scoring_weights=BASE_WEIGHTS,
    ),
    "humanities": ResearchProfile(
        name="humanities",
        preferred_source_types=("book", "peer_reviewed_article", "primary_source"),
        weak_source_types=("social_media",),
        minimum_evidence_count=1,
        recency_requirement="normal",
        citation_style="chicago",
        claim_strictness="medium",
        high_risk_triggers=("attribution", "translation", "archive"),
        required_caveats=(),
        scoring_weights=BASE_WEIGHTS,
    ),
    "policy": ResearchProfile(
        name="policy",
        preferred_source_types=("government_report", "regulator_guidance", "peer_reviewed_article"),
        weak_source_types=("social_media",),
        minimum_evidence_count=2,
        recency_requirement="fast",
        citation_style="harvard_au",
        claim_strictness="high",
        high_risk_triggers=("public health", "national security", "regulation", "election"),
        required_caveats=("Policy conclusions depend on jurisdiction and timing.",),
        scoring_weights=BASE_WEIGHTS,
    ),
    "security": ResearchProfile(
        name="security",
        preferred_source_types=(
            "peer_reviewed_article",
            "conference_paper",
            "standards_document",
            "vendor_advisory",
        ),
        weak_source_types=("blog_post", "social_media"),
        minimum_evidence_count=2,
        recency_requirement="fast",
        citation_style="harvard_au",
        claim_strictness="high",
        high_risk_triggers=(
            "exploit",
            "vulnerability",
            "malware",
            "credential",
            "prompt injection",
            "mcp",
        ),
        required_caveats=(
            "Security findings may change quickly as patches and disclosures evolve.",
        ),
        scoring_weights=BASE_WEIGHTS,
    ),
    "ai_safety": ResearchProfile(
        name="ai_safety",
        preferred_source_types=(
            "peer_reviewed_article",
            "technical_report",
            "model_card",
            "standards_document",
        ),
        weak_source_types=("social_media",),
        minimum_evidence_count=2,
        recency_requirement="fast",
        citation_style="harvard_au",
        claim_strictness="high",
        high_risk_triggers=("alignment", "eval", "dangerous capability", "misuse", "agent"),
        required_caveats=(
            "AI safety findings can change quickly as model capabilities and evaluations evolve.",
        ),
        scoring_weights=BASE_WEIGHTS,
    ),
}

PROFILE_KEYWORDS = {
    "security": (
        "security",
        "vulnerability",
        "malware",
        "credential",
        "prompt injection",
        "mcp",
        "exploit",
    ),
    "ai_safety": ("ai safety", "alignment", "dangerous capability", "eval", "agent"),
    "medicine": ("medical", "clinical", "diagnosis", "treatment", "drug", "patient"),
    "law": ("legal", "law", "contract", "liability", "compliance", "jurisdiction"),
    "business": ("business", "market", "revenue", "investment", "valuation"),
    "engineering": ("engineering", "system design", "safety-critical", "load", "certification"),
    "science": ("science", "experiment", "replication", "dataset", "study"),
    "humanities": ("literature", "archive", "translation", "philosophy"),
    "policy": ("policy", "regulation", "public health", "government", "election"),
}

HIGH_STAKES_TERMS = (
    "medical",
    "clinical",
    "diagnosis",
    "treatment",
    "legal",
    "liability",
    "financial",
    "investment",
    "safety",
    "vulnerability",
    "exploit",
    "credential",
    "prompt injection",
    "public health",
)

VALID_MODES = {"quick", "standard", "deep", "scholarly", "audit", "high_stakes"}


def list_profiles() -> list[ResearchProfile]:
    return [PROFILES[name] for name in sorted(PROFILES)]


def get_profile(name: str) -> ResearchProfile:
    try:
        return PROFILES[name]
    except KeyError as exc:
        raise ValueError(f"unknown profile: {name}") from exc


def select_profile(query: str, requested_profile: str = "auto") -> ResearchProfile:
    if requested_profile != "auto":
        return get_profile(requested_profile)
    lowered = query.lower()
    for profile_name, keywords in PROFILE_KEYWORDS.items():
        if any(keyword in lowered for keyword in keywords):
            return PROFILES[profile_name]
    return PROFILES["general"]


def classify_research_mode(query: str, requested_mode: str = "standard") -> str:
    if requested_mode not in VALID_MODES:
        raise ValueError(f"unknown research mode: {requested_mode}")
    lowered = query.lower()
    if requested_mode == "high_stakes" or any(term in lowered for term in HIGH_STAKES_TERMS):
        return "high_stakes"
    return requested_mode
