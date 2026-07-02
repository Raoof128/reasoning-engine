"""Semantic Scholar adapter — all fields; optional S2_API_KEY for higher limits."""

from __future__ import annotations

import os
from urllib.parse import urlencode

from reasoning_engine.verifiable.sources import base
from reasoning_engine.verifiable.sources.result import RetrievalResult

ENDPOINT = "https://api.semanticscholar.org/graph/v1/paper/search"
FIELDS = "title,abstract,year,authors,venue,externalIds,citationCount,url,publicationTypes"


class SemanticScholarAdapter:
    name = "semantic_scholar"

    def search(self, run_id: str, query: str, limit: int = 10) -> RetrievalResult:
        params = urlencode({"query": query, "limit": limit, "fields": FIELDS})
        headers = {}
        api_key = os.environ.get("S2_API_KEY")
        if api_key:
            headers["x-api-key"] = api_key
        try:
            payload = base.fetch_json(f"{ENDPOINT}?{params}", headers=headers)
        except base.AdapterError as exc:
            return RetrievalResult(evidence=[], error=exc.to_retrieval_error({"source": self.name}))

        items = payload.get("data", []) if isinstance(payload, dict) else []
        evidence = []
        for position, item in enumerate(items, start=1):
            external = item.get("externalIds") or {}
            authors = [a.get("name", "") for a in item.get("authors", [])]
            pub_types = item.get("publicationTypes") or []
            source_type = "preprint" if "Preprint" in pub_types else "peer_reviewed_article"
            evidence.append(
                base.build_record(
                    run_id=run_id,
                    source_adapter=self.name,
                    query=query,
                    rank=position,
                    source_type=source_type,
                    title=item.get("title") or "",
                    authors=[a for a in authors if a],
                    year=base.coerce_year(item.get("year")),
                    venue=item.get("venue", "") or "",
                    doi=external.get("DOI"),
                    url=item.get("url"),
                    score=base.relevance_score(position, limit, item.get("citationCount")),
                    snippet=item.get("abstract") or "",
                    metadata={"cited_by": item.get("citationCount"), "external_ids": external},
                )
            )
        return RetrievalResult(evidence=evidence)
