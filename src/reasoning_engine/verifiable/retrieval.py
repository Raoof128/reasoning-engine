"""Public retrieval surface: adapters, aggregator, and the default factory.

Real academic retrieval lives in ``verifiable.sources`` (one module per
provider) and ``verifiable.aggregator``. This module keeps the historical import
paths stable and exposes ``default_search_adapter()``, which the service uses.
"""

from __future__ import annotations

from reasoning_engine.verifiable.aggregator import AcademicSearchAggregator, default_adapters
from reasoning_engine.verifiable.models import RetrievalError
from reasoning_engine.verifiable.sources.mock import MockScholarGatewayAdapter
from reasoning_engine.verifiable.sources.result import RetrievalResult
from reasoning_engine.verifiable.sources.scholar_gateway import (
    SCHOLAR_GATEWAY_MCP_URL,
    ScholarGatewayAdapter,
)


def default_search_adapter() -> AcademicSearchAggregator:
    """Return the aggregator over all enabled academic sources (or offline mock)."""
    return AcademicSearchAggregator()


__all__ = [
    "SCHOLAR_GATEWAY_MCP_URL",
    "AcademicSearchAggregator",
    "MockScholarGatewayAdapter",
    "RetrievalError",
    "RetrievalResult",
    "ScholarGatewayAdapter",
    "default_adapters",
    "default_search_adapter",
]
