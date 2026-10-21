"""Record academic API responses and the Python adapters' parse of them.

Usage (repo root):
  python3 -I tests/fixtures/record_http.py record    # live network, refreshes bodies
  python3 -I tests/fixtures/record_http.py expected  # offline, rebuilds expected outputs

`record` saves each source's raw response body. `expected` runs the real
Python adapters over every saved body (recorded and hand-written edge cases)
with the network patched out, and writes what they return; the Rust adapters
must produce the same. It also captures Python's request URL for a set of
tricky queries. Only stdlib modules are imported, plus defusedxml for arXiv.
"""

from __future__ import annotations

import json
import os
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "src"))

from reasoning_engine.verifiable.aggregator import AcademicSearchAggregator  # noqa: E402
from reasoning_engine.verifiable.sources import FREE_ADAPTERS, base  # noqa: E402

HTTP_DIR = Path(__file__).with_name("http")
LIVE_QUERY = "reward model"
LIVE_LIMIT = 3

# Hand-written bodies covering the paths tests/test_verifiable_sources.py
# exercises, plus a few more. Keys are "<source>__<case>".
EDGE_CASES: dict[str, str] = {
    "openalex__basic": json.dumps(
        {
            "results": [
                {
                    "title": "Deep Learning for MCP Security",
                    "publication_year": 2025,
                    "doi": "https://doi.org/10.1/abc",
                    "cited_by_count": 42,
                    "authorships": [{"author": {"display_name": "A. Smith"}}, {"author": None}],
                    "primary_location": {
                        "source": {
                            "display_name": "Journal of AI",
                            "host_organization_name": "Elsevier",
                        },
                        "landing_page_url": "https://ex.com/abc",
                    },
                    "abstract_inverted_index": {
                        "This": [0],
                        "studies": [1],
                        "MCP": [2, 4],
                        "x": [3],
                    },
                    "id": "https://openalex.org/W1",
                },
                {"display_name": "Fallback title", "primary_location": None, "id": "W2"},
            ]
        }
    ),
    "crossref__jats": json.dumps(
        {
            "message": {
                "items": [
                    {
                        "title": ["Verifiable Research"],
                        "author": [{"given": "B", "family": "Jones"}, {"family": "Solo"}, {}],
                        "DOI": "10.2/XYZ",
                        "publisher": "ACM",
                        "container-title": ["Proc X"],
                        "abstract": "<jats:p>Great   work</jats:p>",
                        "is-referenced-by-count": 5,
                        "URL": "https://doi.org/10.2/xyz",
                        "issued": {"date-parts": [[2024, 1, 1]]},
                    },
                    {"title": [], "published-online": {"date-parts": [["2019"]]}},
                ]
            }
        }
    ),
    "arxiv__basic": """<?xml version="1.0"?>
<feed xmlns="http://www.w3.org/2005/Atom" xmlns:arxiv="http://arxiv.org/schemas/atom">
  <entry>
    <title>  Attention Is All
      You Need  </title>
    <summary>We propose the Transformer &amp; friends.</summary>
    <published>2017-06-12T00:00:00Z</published>
    <id>http://arxiv.org/abs/1706.03762v1</id>
    <arxiv:doi>10.5/TRANSF</arxiv:doi>
    <author><name>Ashish Vaswani</name></author>
    <author><name> </name></author>
  </entry>
  <entry><title>No extras</title></entry>
</feed>""",
    "arxiv__entities": """<?xml version="1.0"?>
<!DOCTYPE lolz [<!ENTITY lol "lol">
  <!ENTITY lol2 "&lol;&lol;&lol;&lol;">]>
<feed xmlns="http://www.w3.org/2005/Atom"><entry><title>&lol2;</title></entry></feed>""",
    "arxiv__not_xml": "this is not xml <<<",
    "semantic_scholar__basic": json.dumps(
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
                },
                {"title": None, "abstract": None, "publicationTypes": ["Preprint"]},
            ]
        }
    ),
    "europepmc__author_string": json.dumps(
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
                        "pmid": "999",
                    },
                    {"title": "No source", "authorString": None},
                ]
            }
        }
    ),
    "doaj__identifiers": json.dumps(
        {
            "results": [
                {
                    "id": "d1",
                    "bibjson": {
                        "title": "Open Access Study",
                        "author": [{"name": "C. Lee"}],
                        "year": "2022",
                        "identifier": [
                            {"type": "pissn", "id": "1234"},
                            {"type": "DOI", "id": "10.8/oa"},
                        ],
                        "journal": {"title": "OA Journal", "publisher": "OA Pub"},
                        "abstract": "Findings.",
                        "link": [{"type": "fulltext"}, {"url": "https://oa.org/1"}],
                    },
                }
            ]
        }
    ),
    "dblp__author_list": json.dumps(
        {
            "result": {
                "hits": {
                    "hit": [
                        {
                            "info": {
                                "title": "BFT Consensus",
                                "authors": {"author": [{"text": "D. Kim"}, "E. Park"]},
                                "year": "2021",
                                "doi": "10.9/bft",
                                "venue": "OSDI",
                                "ee": "https://dblp.org/1",
                                "type": "Conference and Workshop Papers",
                                "key": "conf/osdi/1",
                            }
                        },
                        {
                            "info": {
                                "title": "Solo Paper",
                                "authors": {"author": {"text": "F. Solo"}},
                                "year": "2020",
                                "type": "Journal Articles",
                                "url": "https://dblp.org/2",
                            }
                        },
                    ]
                }
            }
        }
    ),
    "openalex__not_json": "<html>Service Unavailable</html>",
    "crossref__empty": json.dumps({"message": {"items": []}}),
    "dblp__list_payload": json.dumps([1, 2, 3]),
    # DBLP's documented response shape; the live API challenges datacenter IPs
    # (see dblp__bot_challenge), so this stands in for a recording.
    "dblp__realistic": json.dumps(
        {
            "result": {
                "query": "reward* model*",
                "status": {"@code": "200", "text": "OK"},
                "hits": {
                    "@total": "2",
                    "@computed": "2",
                    "@sent": "2",
                    "@first": "0",
                    "hit": [
                        {
                            "@score": "7",
                            "@id": "1",
                            "info": {
                                "authors": {
                                    "author": [
                                        {"@pid": "1/1", "text": "Hunter Lightman"},
                                        {"@pid": "2/2", "text": "Vineet Kosaraju"},
                                    ]
                                },
                                "title": "Let's Verify Step by Step.",
                                "venue": "ICLR",
                                "year": "2024",
                                "type": "Conference and Workshop Papers",
                                "access": "open",
                                "key": "conf/iclr/LightmanKBEBLLS24",
                                "ee": "https://openreview.net/forum?id=v8L0pN6EOi",
                                "url": "https://dblp.org/rec/conf/iclr/LightmanKBEBLLS24",
                            },
                            "url": "URL#1",
                        },
                        {
                            "@score": "5",
                            "@id": "2",
                            "info": {
                                "authors": {"author": {"@pid": "3/3", "text": "Solo Author"}},
                                "title": "Process Reward Models: A Survey.",
                                "venue": "CoRR",
                                "volume": "abs/2510.08049",
                                "year": "2025",
                                "type": "Informal and Other Publications",
                                "doi": "10.48550/ARXIV.2510.08049",
                                "key": "journals/corr/abs-2510-08049",
                                "url": "https://dblp.org/rec/journals/corr/abs-2510-08049",
                            },
                        },
                    ],
                },
            }
        }
    ),
}

# Live responses captured once and kept verbatim because they document real
# failure modes. Keys are "<source>__<case>"; bodies live in HTTP_DIR.
CAPTURED_CASES = [
    "dblp__bot_challenge",  # DBLP's anti-bot page, served with HTTP 202
]

URL_QUERIES = [
    ("reward model", 3),
    ("MCP prompt-injection: a/b & c?", 10),
    ("café نتیجه", 5),
    ("Does intermittent fasting help in type 2 diabetes?", 5),
]


def _adapter(name: str):
    return FREE_ADAPTERS[name]()


class _Replay:
    """An adapter that returns a fixed result, to feed the real aggregator."""

    def __init__(self, name: str, result) -> None:
        self.name = name
        self._result = result

    def search(self, run_id: str, query: str, limit: int = 10):
        return self._result


def _clean_env() -> None:
    for var in ("REASONING_ENGINE_CONTACT_EMAIL", "S2_API_KEY", "OPENALEX_API_KEY"):
        os.environ.pop(var, None)


def record() -> None:
    _clean_env()
    HTTP_DIR.mkdir(exist_ok=True)
    original = base._request
    manifest = {}
    for name in FREE_ADAPTERS:
        captured: dict = {}

        def capture(url, headers, timeout, _captured=captured):
            _captured["url"] = url
            _captured["body"] = original(url, headers, timeout)
            return _captured["body"]

        base._request = capture
        for attempt in range(3):
            result = _adapter(name).search("run_fixture", LIVE_QUERY, LIVE_LIMIT)
            if result.error is None or result.error.error_type != "rate_limited":
                break
            time.sleep(5 * 2**attempt)
        base._request = original
        if "body" not in captured:
            # Keyless Semantic Scholar is often rate-limited from shared IPs.
            # Its parsing stays covered by the hand-written edge case.
            print(f"WARNING {name}: skipped, no body recorded ({result.error})")
            continue
        if result.error is not None:
            # A 2xx body the adapter cannot parse (e.g. an anti-bot page) is a
            # failure mode, not a fixture. Save it for review, do not record it.
            (HTTP_DIR / f"{name}__live_error.body").write_bytes(captured["body"])
            print(
                f"WARNING {name}: live body rejected ({result.error.error_type}), saved for review"
            )
            continue
        (HTTP_DIR / f"{name}.body").write_bytes(captured["body"])
        manifest[name] = {"query": LIVE_QUERY, "limit": LIVE_LIMIT}
        print(f"{name}: {len(captured['body'])} bytes, {len(result.evidence)} records")
    (HTTP_DIR / "recorded.json").write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")


def _strip_volatile(record: dict) -> dict:
    record = dict(record)
    record.pop("retrieved_at")
    return record


def expected() -> None:
    _clean_env()
    recorded = json.loads((HTTP_DIR / "recorded.json").read_text())
    cases = {
        f"{name}__recorded": (name, spec["query"], spec["limit"]) for name, spec in recorded.items()
    }
    for key in [*EDGE_CASES, *CAPTURED_CASES]:
        cases[key] = (key.split("__")[0], "edge query", 5)

    out = {}
    results = {}
    for key, (name, query, limit) in sorted(cases.items()):
        body_path = HTTP_DIR / (f"{name}.body" if key.endswith("__recorded") else f"{key}.body")
        if key in EDGE_CASES:
            body_path.write_text(EDGE_CASES[key], encoding="utf-8")
        body = body_path.read_bytes()
        seen: dict = {}

        def fake_request(url, headers, timeout, _seen=seen, _body=body):
            _seen["url"] = url
            return _body

        base._request = fake_request
        result = _adapter(name).search("run_fixture", query, limit)
        results[key] = result
        out[key] = {
            "source": name,
            "body": body_path.name,
            "query": query,
            "limit": limit,
            "url": seen.get("url"),
            "evidence": [_strip_volatile(e.to_dict()) for e in result.evidence],
            "error": None if result.error is None else result.error.to_dict(),
        }

    urls = []
    for name in FREE_ADAPTERS:
        for query, limit in URL_QUERIES:
            seen = {}

            def fake_request(url, headers, timeout, _seen=seen):
                _seen["url"] = url
                raise base.AdapterError("unavailable", "url capture only", retryable=True)

            base._request = fake_request
            _adapter(name).search("run_fixture", query, limit)
            urls.append({"source": name, "query": query, "limit": limit, "url": seen["url"]})

    def aggregate(keys: list[str], limit: int) -> dict:
        adapters = [_Replay(cases[key][0], results[key]) for key in keys]
        result = AcademicSearchAggregator(adapters=adapters).search("run_fixture", "q", limit)
        return {
            "keys": keys,
            "limit": limit,
            "evidence": [_strip_volatile(e.to_dict()) for e in result.evidence],
            "error": None if result.error is None else result.error.to_dict(),
        }

    keys = sorted(cases)
    aggregates = {
        "recorded_top10": aggregate([k for k in keys if k.endswith("__recorded")], 10),
        "everything": aggregate(keys, 50),
        "all_fail": aggregate([k for k in keys if results[k].error is not None], 10),
    }

    (HTTP_DIR / "expected.json").write_text(
        json.dumps({"aggregates": aggregates, "cases": out, "urls": urls}, indent=2, sort_keys=True)
        + "\n"
    )
    print(f"wrote {len(out)} cases, {len(urls)} URLs and {len(aggregates)} aggregates")


if __name__ == "__main__":
    command = sys.argv[1] if len(sys.argv) > 1 else ""
    if command == "record":
        record()
    elif command == "expected":
        expected()
    else:
        sys.exit("usage: record_http.py record|expected")
