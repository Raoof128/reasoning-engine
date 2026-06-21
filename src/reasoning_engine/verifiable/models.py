"""Shared data models for verifiable research workflows."""

from __future__ import annotations

import hashlib
import json
import uuid
from dataclasses import asdict, dataclass, field
from datetime import UTC, datetime
from typing import Any

SUPPORT_STATUSES = {
    "supported",
    "partially_supported",
    "contradicted",
    "unsupported",
    "needs_more_evidence",
    "hypothesis",
    "not_verifiable",
    "out_of_scope",
}

CLAIM_TYPES = {
    "empirical",
    "definitional",
    "causal",
    "comparative",
    "quantitative",
    "historical",
    "legal",
    "medical",
    "security",
    "policy",
    "predictive",
    "normative",
    "methodological",
    "hypothesis",
}

RETRIEVAL_ERROR_TYPES = {
    "auth_required",
    "rate_limited",
    "unavailable",
    "malformed_response",
    "empty_result",
    "licence_restricted",
    "unsupported_query",
    "adapter_misconfigured",
}


def utc_now() -> str:
    return datetime.now(UTC).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def stable_json_hash(value: Any) -> str:
    encoded = json.dumps(value, sort_keys=True, separators=(",", ":")).encode()
    return f"sha256:{hashlib.sha256(encoded).hexdigest()}"


def normalize_status(status: str) -> str:
    if status not in SUPPORT_STATUSES:
        raise ValueError(f"unknown support status: {status}")
    return status


def normalize_claim_type(claim_type: str) -> str:
    if claim_type not in CLAIM_TYPES:
        raise ValueError(f"unknown claim type: {claim_type}")
    return claim_type


@dataclass(frozen=True)
class ResearchRun:
    run_id: str
    query: str
    profile: str
    mode: str
    created_at: str
    status: str = "active"

    @classmethod
    def create(cls, query: str, profile: str, mode: str) -> ResearchRun:
        return cls(
            run_id=f"run_{uuid.uuid4().hex[:12]}",
            query=query,
            profile=profile,
            mode=mode,
            created_at=utc_now(),
        )

    def to_dict(self) -> dict[str, Any]:
        return asdict(self)


@dataclass(frozen=True)
class EvidenceRecord:
    evidence_id: str
    run_id: str
    source_adapter: str
    source_type: str
    title: str
    authors: list[str]
    year: int | None
    publisher: str
    venue: str
    doi: str | None
    url: str | None
    retrieved_at: str
    query: str
    rank: int
    score: float
    snippet: str
    licence_notes: str | None
    risk_flags: list[str]
    metadata: dict[str, Any]

    @property
    def snippet_hash(self) -> str:
        return f"sha256:{hashlib.sha256(self.snippet.encode()).hexdigest()}"

    def to_dict(self) -> dict[str, Any]:
        payload = asdict(self)
        payload["snippet_hash"] = self.snippet_hash
        return payload


@dataclass
class ClaimRecord:
    claim_id: str
    run_id: str
    text: str
    claim_type: str
    domain: str
    importance: str
    risk_level: str
    requires_citation: bool
    created_from: str
    status: str = "needs_more_evidence"
    evidence_ids: list[str] = field(default_factory=list)
    contradiction_ids: list[str] = field(default_factory=list)
    confidence: float | None = None
    human_review_required: bool = False

    def __post_init__(self) -> None:
        normalize_claim_type(self.claim_type)
        normalize_status(self.status)

    def to_dict(self) -> dict[str, Any]:
        return asdict(self)


@dataclass(frozen=True)
class VerificationRecord:
    verification_id: str
    claim_id: str
    method: str
    evidence_ids: list[str]
    support_status: str
    support_rationale: str
    missing_evidence: str | None
    contradictory_evidence_ids: list[str]
    confidence: float
    requires_human_review: bool
    verified_at: str

    def __post_init__(self) -> None:
        normalize_status(self.support_status)

    def to_dict(self) -> dict[str, Any]:
        return asdict(self)


@dataclass(frozen=True)
class EvidenceGap:
    gap_id: str
    run_id: str
    query: str
    reason: str
    created_at: str

    def to_dict(self) -> dict[str, Any]:
        return asdict(self)


@dataclass(frozen=True)
class RetrievalError:
    error_type: str
    message: str
    retryable: bool
    metadata: dict[str, Any]

    def __post_init__(self) -> None:
        if self.error_type not in RETRIEVAL_ERROR_TYPES:
            raise ValueError(f"unknown retrieval error type: {self.error_type}")

    def to_dict(self) -> dict[str, Any]:
        return asdict(self)
