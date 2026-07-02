"""Aggregator dedup, ranking, error handling, and source-selection tests."""

from __future__ import annotations

from reasoning_engine.verifiable.aggregator import AcademicSearchAggregator, default_adapters
from reasoning_engine.verifiable.models import RetrievalError
from reasoning_engine.verifiable.sources.base import build_record
from reasoning_engine.verifiable.sources.result import RetrievalResult


class FakeAdapter:
    def __init__(self, name, result):
        self.name = name
        self._result = result

    def search(self, run_id, query, limit=10):
        return self._result


def _rec(name, *, doi=None, title="Untitled", score=0.5, rank=1):
    return build_record(
        run_id="run_1",
        source_adapter=name,
        query="q",
        rank=rank,
        doi=doi,
        title=title,
        score=score,
    )


def test_aggregator_dedups_by_doi_and_keeps_higher_score():
    a = FakeAdapter(
        "openalex", RetrievalResult(evidence=[_rec("openalex", doi="10.1/x", score=0.9)])
    )
    b = FakeAdapter(
        "crossref", RetrievalResult(evidence=[_rec("crossref", doi="10.1/x", score=0.4)])
    )
    result = AcademicSearchAggregator(adapters=[a, b]).search("run_1", "q", limit=10)
    assert len(result.evidence) == 1
    kept = result.evidence[0]
    assert kept.source_adapter == "openalex"
    assert "crossref" in kept.metadata.get("also_found_in", [])


def test_aggregator_dedups_by_title_when_no_doi():
    a = FakeAdapter(
        "arxiv", RetrievalResult(evidence=[_rec("arxiv", title="Same Title!", score=0.3)])
    )
    b = FakeAdapter("dblp", RetrievalResult(evidence=[_rec("dblp", title="same title", score=0.7)]))
    result = AcademicSearchAggregator(adapters=[a, b]).search("run_1", "q", limit=10)
    assert len(result.evidence) == 1
    assert result.evidence[0].source_adapter == "dblp"


def test_aggregator_ranks_and_renumbers_evidence_ids():
    a = FakeAdapter(
        "openalex", RetrievalResult(evidence=[_rec("openalex", doi="10.1/a", score=0.2)])
    )
    b = FakeAdapter(
        "crossref", RetrievalResult(evidence=[_rec("crossref", doi="10.2/b", score=0.8)])
    )
    result = AcademicSearchAggregator(adapters=[a, b]).search("run_1", "q", limit=10)
    assert [e.evidence_id for e in result.evidence] == ["ev_0001", "ev_0002"]
    assert [e.rank for e in result.evidence] == [1, 2]
    assert result.evidence[0].score == 0.8  # highest first


def test_aggregator_truncates_to_limit():
    evidence = [_rec("openalex", doi=f"10.1/{i}", score=1.0 - i * 0.01) for i in range(10)]
    a = FakeAdapter("openalex", RetrievalResult(evidence=evidence))
    result = AcademicSearchAggregator(adapters=[a]).search("run_1", "q", limit=3)
    assert len(result.evidence) == 3


def test_aggregator_tolerates_partial_failure():
    good = FakeAdapter(
        "openalex", RetrievalResult(evidence=[_rec("openalex", doi="10.1/x", score=0.5)])
    )
    bad = FakeAdapter(
        "crossref",
        RetrievalResult(evidence=[], error=RetrievalError("unavailable", "down", True, {})),
    )
    result = AcademicSearchAggregator(adapters=[good, bad]).search("run_1", "q", limit=10)
    assert result.error is None
    assert len(result.evidence) == 1


def test_aggregator_surfaces_error_when_all_fail():
    a = FakeAdapter(
        "openalex",
        RetrievalResult(evidence=[], error=RetrievalError("rate_limited", "429", True, {})),
    )
    b = FakeAdapter(
        "crossref",
        RetrievalResult(evidence=[], error=RetrievalError("unavailable", "down", True, {})),
    )
    result = AcademicSearchAggregator(adapters=[a, b]).search("run_1", "q", limit=10)
    assert result.evidence == []
    assert result.error is not None
    assert result.error.error_type == "rate_limited"  # actionable type preferred
    assert set(result.error.metadata["failed_sources"]) == {"openalex", "crossref"}


def test_aggregator_rejects_empty_query():
    result = AcademicSearchAggregator(
        adapters=[FakeAdapter("x", RetrievalResult(evidence=[]))]
    ).search("run_1", "   ", limit=10)
    assert result.error is not None
    assert result.error.error_type == "unsupported_query"


def test_aggregator_survives_crashing_adapter():
    class Crashing:
        name = "boom"

        def search(self, run_id, query, limit=10):
            raise RuntimeError("kaboom")

    good = FakeAdapter(
        "openalex", RetrievalResult(evidence=[_rec("openalex", doi="10.1/x", score=0.5)])
    )
    result = AcademicSearchAggregator(adapters=[Crashing(), good]).search("run_1", "q", limit=10)
    assert len(result.evidence) == 1


def test_default_adapters_offline_returns_mock(monkeypatch):
    monkeypatch.setenv("REASONING_ENGINE_OFFLINE", "1")
    adapters = default_adapters()
    assert len(adapters) == 1
    assert adapters[0].name == "scholar_gateway"


def test_default_adapters_honours_source_selection(monkeypatch):
    monkeypatch.delenv("REASONING_ENGINE_OFFLINE", raising=False)
    monkeypatch.setenv("REASONING_ENGINE_SOURCES", "openalex,crossref")
    names = sorted(a.name for a in default_adapters())
    assert names == ["crossref", "openalex"]


def test_default_adapters_defaults_to_all_free(monkeypatch):
    monkeypatch.delenv("REASONING_ENGINE_OFFLINE", raising=False)
    monkeypatch.delenv("REASONING_ENGINE_SOURCES", raising=False)
    monkeypatch.delenv("SCHOLAR_GATEWAY_LIVE", raising=False)
    names = {a.name for a in default_adapters()}
    assert names == {
        "openalex",
        "crossref",
        "arxiv",
        "semantic_scholar",
        "europepmc",
        "doaj",
        "dblp",
    }
