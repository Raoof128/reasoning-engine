"""Academic source adapters and shared retrieval plumbing."""

from reasoning_engine.verifiable.sources.arxiv import ArxivAdapter
from reasoning_engine.verifiable.sources.crossref import CrossrefAdapter
from reasoning_engine.verifiable.sources.dblp import DblpAdapter
from reasoning_engine.verifiable.sources.doaj import DoajAdapter
from reasoning_engine.verifiable.sources.europepmc import EuropePmcAdapter
from reasoning_engine.verifiable.sources.mock import MockScholarGatewayAdapter
from reasoning_engine.verifiable.sources.openalex import OpenAlexAdapter
from reasoning_engine.verifiable.sources.result import RetrievalResult
from reasoning_engine.verifiable.sources.scholar_gateway import ScholarGatewayAdapter
from reasoning_engine.verifiable.sources.semantic_scholar import SemanticScholarAdapter

# Free, no-auth adapters keyed by their canonical name.
FREE_ADAPTERS = {
    "openalex": OpenAlexAdapter,
    "crossref": CrossrefAdapter,
    "arxiv": ArxivAdapter,
    "semantic_scholar": SemanticScholarAdapter,
    "europepmc": EuropePmcAdapter,
    "doaj": DoajAdapter,
    "dblp": DblpAdapter,
}

__all__ = [
    "FREE_ADAPTERS",
    "ArxivAdapter",
    "CrossrefAdapter",
    "DblpAdapter",
    "DoajAdapter",
    "EuropePmcAdapter",
    "MockScholarGatewayAdapter",
    "OpenAlexAdapter",
    "RetrievalResult",
    "ScholarGatewayAdapter",
    "SemanticScholarAdapter",
]
