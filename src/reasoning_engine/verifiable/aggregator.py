"""Fan-out academic search aggregator with dedup and deterministic ranking."""

from __future__ import annotations

import dataclasses
import os
from concurrent.futures import ThreadPoolExecutor
from typing import TYPE_CHECKING

from reasoning_engine.verifiable.models import RetrievalError
from reasoning_engine.verifiable.sources import FREE_ADAPTERS
from reasoning_engine.verifiable.sources.mock import MockScholarGatewayAdapter
from reasoning_engine.verifiable.sources.result import RetrievalResult
from reasoning_engine.verifiable.sources.scholar_gateway import ScholarGatewayAdapter

if TYPE_CHECKING:
    from reasoning_engine.verifiable.models import EvidenceRecord
    from reasoning_engine.verifiable.sources.base import SourceAdapter

MAX_WORKERS = 8


def _normalized_title(title: str) -> str:
    return "".join(char for char in title.lower() if char.isalnum())


def _dedup_key(record: EvidenceRecord) -> str:
    if record.doi:
        return f"doi:{record.doi}"
    return f"title:{_normalized_title(record.title)}"


def default_adapters() -> list[SourceAdapter]:
    """Build the enabled adapter list from env, defaulting to all free sources."""
    if os.environ.get("REASONING_ENGINE_OFFLINE") == "1":
        # Deterministic, network-free fixtures for tests/CI/no-network runs.
        return [MockScholarGatewayAdapter()]
    requested = os.environ.get("REASONING_ENGINE_SOURCES", "").strip()
    if requested:
        names = [name.strip() for name in requested.split(",") if name.strip()]
    else:
        names = list(FREE_ADAPTERS)
    adapters: list[SourceAdapter] = [
        FREE_ADAPTERS[name]() for name in names if name in FREE_ADAPTERS
    ]
    # Scholar Gateway participates only when it is actually configured, so it
    # never pollutes live results with its offline mock fixtures.
    if ScholarGatewayAdapter.is_configured() and (not requested or "scholar_gateway" in names):
        adapters.append(ScholarGatewayAdapter())
    return adapters


class AcademicSearchAggregator:
    name = "academic_aggregator"

    def __init__(self, adapters: list[SourceAdapter] | None = None):
        self._adapters = adapters if adapters is not None else default_adapters()

    def search(self, run_id: str, query: str, limit: int = 10) -> RetrievalResult:
        normalized_query = query.strip()
        if not normalized_query:
            return RetrievalResult(
                evidence=[],
                error=RetrievalError(
                    "unsupported_query", "query must not be empty", retryable=False, metadata={}
                ),
            )
        if not self._adapters:
            return RetrievalResult(
                evidence=[],
                error=RetrievalError(
                    "adapter_misconfigured",
                    "no search adapters are enabled",
                    retryable=False,
                    metadata={},
                ),
            )

        results = self._run_adapters(run_id, normalized_query, limit)

        collected: list[EvidenceRecord] = []
        errors: dict[str, RetrievalError] = {}
        for adapter, result in results:
            if result.error is not None:
                errors[adapter.name] = result.error
            collected.extend(result.evidence)

        if not collected:
            return RetrievalResult(evidence=[], error=self._aggregate_error(errors))

        merged = self._dedup(collected)
        merged.sort(key=lambda rec: (-rec.score, rec.source_adapter, _dedup_key(rec)))
        final = [
            dataclasses.replace(record, evidence_id=f"ev_{index:04d}", rank=index)
            for index, record in enumerate(merged[:limit], start=1)
        ]
        return RetrievalResult(evidence=final)

    def _run_adapters(self, run_id, query, limit) -> list[tuple[SourceAdapter, RetrievalResult]]:
        def run(adapter: SourceAdapter) -> tuple[SourceAdapter, RetrievalResult]:
            try:
                return adapter, adapter.search(run_id, query, limit)
            except Exception as exc:  # defensive: one adapter must never crash the fan-out
                return adapter, RetrievalResult(
                    evidence=[],
                    error=RetrievalError(
                        "unavailable", f"{adapter.name} adapter crashed: {exc}", True, {}
                    ),
                )

        workers = min(MAX_WORKERS, len(self._adapters))
        with ThreadPoolExecutor(max_workers=workers) as pool:
            return list(pool.map(run, self._adapters))

    @staticmethod
    def _dedup(records: list[EvidenceRecord]) -> list[EvidenceRecord]:
        best: dict[str, EvidenceRecord] = {}
        for record in records:
            key = _dedup_key(record)
            existing = best.get(key)
            if existing is None or record.score > existing.score:
                if existing is not None:
                    also = list(existing.metadata.get("also_found_in", []))
                    also.append(existing.source_adapter)
                    record = dataclasses.replace(
                        record, metadata={**record.metadata, "also_found_in": also}
                    )
                best[key] = record
            else:
                also = list(existing.metadata.get("also_found_in", []))
                also.append(record.source_adapter)
                best[key] = dataclasses.replace(
                    existing, metadata={**existing.metadata, "also_found_in": also}
                )
        return list(best.values())

    @staticmethod
    def _aggregate_error(errors: dict[str, RetrievalError]) -> RetrievalError | None:
        if not errors:
            return None
        # Prefer a specific, actionable error type when every source agrees.
        for preferred in ("auth_required", "rate_limited", "malformed_response"):
            if any(err.error_type == preferred for err in errors.values()):
                match = next(err for err in errors.values() if err.error_type == preferred)
                return dataclasses.replace(
                    match, metadata={**match.metadata, "failed_sources": sorted(errors)}
                )
        representative = next(iter(errors.values()))
        return RetrievalError(
            error_type=representative.error_type,
            message=f"all sources failed ({', '.join(sorted(errors))})",
            retryable=any(err.retryable for err in errors.values()),
            metadata={"failed_sources": sorted(errors)},
        )
