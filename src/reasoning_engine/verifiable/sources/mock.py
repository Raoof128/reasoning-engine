"""Deterministic mock adapter used for offline/CI runs and tests."""

from __future__ import annotations

from reasoning_engine.verifiable.models import EvidenceRecord, RetrievalError, utc_now
from reasoning_engine.verifiable.sources.result import RetrievalResult


class MockScholarGatewayAdapter:
    name = "scholar_gateway"

    def search(self, run_id: str, query: str, limit: int = 10) -> RetrievalResult:
        normalized_query = query.strip()
        if not normalized_query:
            return RetrievalResult(
                evidence=[],
                error=RetrievalError(
                    "unsupported_query",
                    "query must not be empty",
                    retryable=False,
                    metadata={},
                ),
            )
        evidence = [
            EvidenceRecord(
                evidence_id=f"ev_{index:04d}",
                run_id=run_id,
                source_adapter="scholar_gateway",
                source_type="peer_reviewed_article",
                title=f"Mock Scholar Gateway Result {index}",
                authors=["Mock Author"],
                year=2026,
                publisher="Wiley",
                venue="Mock Journal",
                doi=f"10.1000/mock.{index}",
                url=f"https://doi.org/10.1000/mock.{index}",
                retrieved_at=utc_now(),
                query=normalized_query,
                rank=index,
                score=max(0.0, 1.0 - (index - 1) * 0.05),
                snippet=f"Mock evidence for {normalized_query}.",
                licence_notes="Mock fixture for tests.",
                risk_flags=[],
                metadata={"mock": True},
            )
            for index in range(1, limit + 1)
        ]
        return RetrievalResult(evidence=evidence)
