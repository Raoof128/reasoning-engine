"""Parser tests for each academic source adapter (no live network)."""

from __future__ import annotations

import pytest

from reasoning_engine.verifiable.sources import base
from reasoning_engine.verifiable.sources.arxiv import ArxivAdapter
from reasoning_engine.verifiable.sources.crossref import CrossrefAdapter
from reasoning_engine.verifiable.sources.dblp import DblpAdapter
from reasoning_engine.verifiable.sources.doaj import DoajAdapter
from reasoning_engine.verifiable.sources.europepmc import EuropePmcAdapter
from reasoning_engine.verifiable.sources.openalex import OpenAlexAdapter
from reasoning_engine.verifiable.sources.semantic_scholar import SemanticScholarAdapter


def _patch_json(monkeypatch, payload):
    monkeypatch.setattr(base, "fetch_json", lambda url, **kwargs: payload)


def _patch_text(monkeypatch, payload):
    monkeypatch.setattr(base, "fetch_text", lambda url, **kwargs: payload)


def test_openalex_parses_and_reconstructs_abstract(monkeypatch):
    _patch_json(
        monkeypatch,
        {
            "results": [
                {
                    "title": "Deep Learning for MCP Security",
                    "publication_year": 2025,
                    "doi": "https://doi.org/10.1/abc",
                    "cited_by_count": 42,
                    "authorships": [{"author": {"display_name": "A. Smith"}}],
                    "primary_location": {
                        "source": {
                            "display_name": "Journal of AI",
                            "host_organization_name": "Elsevier",
                        },
                        "landing_page_url": "https://ex.com/abc",
                    },
                    "abstract_inverted_index": {"This": [0], "studies": [1], "MCP": [2]},
                    "id": "https://openalex.org/W1",
                }
            ]
        },
    )
    result = OpenAlexAdapter().search("run_1", "mcp security", limit=5)
    assert result.error is None
    rec = result.evidence[0]
    assert rec.source_adapter == "openalex"
    assert rec.title == "Deep Learning for MCP Security"
    assert rec.doi == "10.1/abc"
    assert rec.year == 2025
    assert rec.authors == ["A. Smith"]
    assert rec.venue == "Journal of AI"
    assert rec.snippet == "This studies MCP"
    assert rec.score > 0


def test_crossref_strips_jats_and_builds_author(monkeypatch):
    _patch_json(
        monkeypatch,
        {
            "message": {
                "items": [
                    {
                        "title": ["Verifiable Research"],
                        "author": [{"given": "B", "family": "Jones"}],
                        "DOI": "10.2/XYZ",
                        "publisher": "ACM",
                        "container-title": ["Proc X"],
                        "abstract": "<jats:p>Great work</jats:p>",
                        "is-referenced-by-count": 5,
                        "URL": "https://doi.org/10.2/xyz",
                        "issued": {"date-parts": [[2024, 1, 1]]},
                    }
                ]
            }
        },
    )
    rec = CrossrefAdapter().search("run_1", "verifiable", limit=5).evidence[0]
    assert rec.title == "Verifiable Research"
    assert rec.doi == "10.2/xyz"
    assert rec.authors == ["B Jones"]
    assert rec.year == 2024
    assert rec.venue == "Proc X"
    assert "Great work" in rec.snippet
    assert "<jats:p>" not in rec.snippet


def test_arxiv_parses_atom_feed(monkeypatch):
    _patch_text(
        monkeypatch,
        """<?xml version="1.0"?>
        <feed xmlns="http://www.w3.org/2005/Atom"
              xmlns:arxiv="http://arxiv.org/schemas/atom">
          <entry>
            <title>Attention Is All You Need</title>
            <summary>We propose the Transformer.</summary>
            <published>2017-06-12T00:00:00Z</published>
            <id>http://arxiv.org/abs/1706.03762v1</id>
            <arxiv:doi>10.5/transf</arxiv:doi>
            <author><name>Ashish Vaswani</name></author>
          </entry>
        </feed>""",
    )
    rec = ArxivAdapter().search("run_1", "transformer", limit=5).evidence[0]
    assert rec.source_type == "preprint"
    assert rec.title == "Attention Is All You Need"
    assert rec.year == 2017
    assert "Transformer" in rec.snippet
    assert rec.doi == "10.5/transf"
    assert rec.url == "http://arxiv.org/abs/1706.03762v1"


def test_arxiv_rejects_malicious_xml(monkeypatch):
    # Billion-laughs style entity payload must be refused, not expanded.
    _patch_text(
        monkeypatch,
        """<?xml version="1.0"?>
        <!DOCTYPE lolz [<!ENTITY lol "lol">
          <!ENTITY lol2 "&lol;&lol;&lol;&lol;">]>
        <feed xmlns="http://www.w3.org/2005/Atom"><entry><title>&lol2;</title></entry></feed>""",
    )
    result = ArxivAdapter().search("run_1", "x", limit=5)
    assert result.evidence == []
    assert result.error is not None
    assert result.error.error_type == "malformed_response"


def test_semantic_scholar_parses(monkeypatch):
    _patch_json(
        monkeypatch,
        {
            "data": [
                {
                    "title": "SimPO",
                    "abstract": "Reference-free preference optimization.",
                    "year": 2024,
                    "authors": [{"name": "Yu Meng"}],
                    "venue": "NeurIPS",
                    "externalIds": {"DOI": "10.6/simpo"},
                    "citationCount": 100,
                    "url": "https://s2.org/1",
                    "publicationTypes": ["JournalArticle"],
                }
            ]
        },
    )
    rec = SemanticScholarAdapter().search("run_1", "preference optimization", limit=5).evidence[0]
    assert rec.title == "SimPO"
    assert rec.doi == "10.6/simpo"
    assert rec.venue == "NeurIPS"
    assert rec.source_type == "peer_reviewed_article"


def test_europepmc_parses_author_string(monkeypatch):
    _patch_json(
        monkeypatch,
        {
            "resultList": {
                "result": [
                    {
                        "title": "CRISPR review",
                        "authorString": "Doe J, Roe K.",
                        "pubYear": "2023",
                        "doi": "10.7/crispr",
                        "journalInfo": {"journal": {"title": "Cell"}},
                        "abstractText": "A review.",
                        "citedByCount": 12,
                        "source": "MED",
                        "id": "999",
                    }
                ]
            }
        },
    )
    rec = EuropePmcAdapter().search("run_1", "crispr", limit=5).evidence[0]
    assert rec.authors == ["Doe J", "Roe K"]
    assert rec.venue == "Cell"
    assert rec.doi == "10.7/crispr"
    assert rec.url == "https://europepmc.org/abstract/MED/999"


def test_doaj_parses_identifiers_and_links(monkeypatch):
    _patch_json(
        monkeypatch,
        {
            "results": [
                {
                    "id": "d1",
                    "bibjson": {
                        "title": "Open Access Study",
                        "author": [{"name": "C. Lee"}],
                        "year": "2022",
                        "identifier": [{"type": "doi", "id": "10.8/oa"}],
                        "journal": {"title": "OA Journal", "publisher": "OA Pub"},
                        "abstract": "Findings.",
                        "link": [{"url": "https://oa.org/1"}],
                    },
                }
            ]
        },
    )
    rec = DoajAdapter().search("run_1", "open access", limit=5).evidence[0]
    assert rec.title == "Open Access Study"
    assert rec.doi == "10.8/oa"
    assert rec.venue == "OA Journal"
    assert rec.url == "https://oa.org/1"
    assert "Open access" in (rec.licence_notes or "")


def test_dblp_parses_type_and_author_list(monkeypatch):
    _patch_json(
        monkeypatch,
        {
            "result": {
                "hits": {
                    "hit": [
                        {
                            "info": {
                                "title": "BFT Consensus",
                                "authors": {"author": [{"text": "D. Kim"}, {"text": "E. Park"}]},
                                "year": "2021",
                                "doi": "10.9/bft",
                                "venue": "OSDI",
                                "ee": "https://dblp.org/1",
                                "type": "Conference and Workshop Papers",
                                "key": "conf/osdi/1",
                            }
                        }
                    ]
                }
            }
        },
    )
    rec = DblpAdapter().search("run_1", "consensus", limit=5).evidence[0]
    assert rec.source_type == "conference_paper"
    assert rec.authors == ["D. Kim", "E. Park"]
    assert rec.doi == "10.9/bft"
    assert rec.venue == "OSDI"


def test_dblp_handles_single_author_dict(monkeypatch):
    _patch_json(
        monkeypatch,
        {
            "result": {
                "hits": {
                    "hit": [
                        {
                            "info": {
                                "title": "Solo Paper",
                                "authors": {"author": {"text": "F. Solo"}},
                                "year": "2020",
                                "type": "Journal Articles",
                            }
                        }
                    ]
                }
            }
        },
    )
    rec = DblpAdapter().search("run_1", "solo", limit=5).evidence[0]
    assert rec.authors == ["F. Solo"]
    assert rec.source_type == "peer_reviewed_article"


@pytest.mark.parametrize(
    "adapter",
    [OpenAlexAdapter(), CrossrefAdapter(), SemanticScholarAdapter(), EuropePmcAdapter()],
)
def test_adapters_map_http_errors_to_retrieval_error(monkeypatch, adapter):
    def boom(url, **kwargs):
        raise base.AdapterError("rate_limited", "HTTP 429 from provider", retryable=True)

    monkeypatch.setattr(base, "fetch_json", boom)
    result = adapter.search("run_1", "q", limit=3)
    assert result.evidence == []
    assert result.error is not None
    assert result.error.error_type == "rate_limited"
