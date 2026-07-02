"""Result container shared by every retrieval adapter and the aggregator."""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from reasoning_engine.verifiable.models import EvidenceRecord, RetrievalError


@dataclass(frozen=True)
class RetrievalResult:
    evidence: list[EvidenceRecord]
    error: RetrievalError | None = None
