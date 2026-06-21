"""Claim extraction and evidence-based MVP verification."""

from __future__ import annotations

import re
import uuid

from reasoning_engine.verifiable.models import (
    ClaimRecord,
    EvidenceRecord,
    VerificationRecord,
    utc_now,
)

WORD_RE = re.compile(r"[a-z0-9]+")


def _claim_type(text: str) -> str:
    lowered = text.lower()
    if re.search(r"\d", text):
        return "quantitative"
    if any(term in lowered for term in ("causes", "caused", "leads to", "because")):
        return "causal"
    if any(term in lowered for term in ("should", "must", "ought")):
        return "normative"
    if any(term in lowered for term in ("may", "might", "could", "likely")):
        return "hypothesis"
    return "empirical"


def extract_claims(run_id: str, text: str, domain: str) -> list[ClaimRecord]:
    sentences = [part.strip() for part in re.split(r"(?<=[.!?])\s+", text.strip()) if part.strip()]
    claims: list[ClaimRecord] = []
    for index, sentence in enumerate(sentences, start=1):
        claim_type = _claim_type(sentence)
        claims.append(
            ClaimRecord(
                claim_id=f"claim_{index:04d}",
                run_id=run_id,
                text=sentence.rstrip(".!?"),
                claim_type=claim_type,
                domain=domain,
                importance="high" if claim_type in {"quantitative", "causal"} else "medium",
                risk_level="medium" if domain in {"security", "medicine", "law"} else "low",
                requires_citation=claim_type not in {"normative", "hypothesis"},
                created_from="draft",
            )
        )
    return claims


def _overlap_score(claim_text: str, snippet: str) -> float:
    claim_terms = {term for term in WORD_RE.findall(claim_text.lower()) if len(term) > 3}
    snippet_terms = {term for term in WORD_RE.findall(snippet.lower()) if len(term) > 3}
    if not claim_terms:
        return 0.0
    return len(claim_terms & snippet_terms) / len(claim_terms)


def verify_claims(
    claims: list[ClaimRecord],
    evidence: list[EvidenceRecord],
) -> list[VerificationRecord]:
    verifications: list[VerificationRecord] = []
    for claim in claims:
        scored = sorted(
            ((_overlap_score(claim.text, item.snippet), item) for item in evidence),
            key=lambda pair: pair[0],
            reverse=True,
        )
        best_score, best_evidence = scored[0] if scored else (0.0, None)
        if not claim.requires_citation:
            status = "hypothesis" if claim.claim_type == "hypothesis" else "not_verifiable"
            evidence_ids: list[str] = []
            rationale = "Claim does not require factual citation support."
            missing = None
        elif best_evidence is None:
            status = "needs_more_evidence"
            evidence_ids = []
            rationale = "No evidence was available for this claim."
            missing = "Retrieve source evidence that directly supports the claim."
        elif best_score >= 0.65:
            status = "supported"
            evidence_ids = [best_evidence.evidence_id]
            rationale = "Evidence snippet overlaps the claim strongly enough for MVP support."
            missing = None
        elif best_score >= 0.35:
            status = "partially_supported"
            evidence_ids = [best_evidence.evidence_id]
            rationale = "Evidence is relevant but does not support the exact claim."
            missing = "Find direct evidence for the full claim."
        else:
            status = "unsupported"
            evidence_ids = []
            rationale = "Available evidence does not support the claim."
            missing = "Find direct evidence or remove the claim from final mode."

        claim.status = status
        claim.evidence_ids = evidence_ids
        claim.confidence = round(best_score, 2)
        verifications.append(
            VerificationRecord(
                verification_id=f"ver_{uuid.uuid4().hex[:12]}",
                claim_id=claim.claim_id,
                method="term_overlap_mvp",
                evidence_ids=evidence_ids,
                support_status=status,
                support_rationale=rationale,
                missing_evidence=missing,
                contradictory_evidence_ids=[],
                confidence=round(best_score, 2),
                requires_human_review=status
                in {"partially_supported", "unsupported", "needs_more_evidence"},
                verified_at=utc_now(),
            )
        )
    return verifications
