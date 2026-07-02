"""DOAJ adapter — Directory of Open Access Journals articles, no auth."""

from __future__ import annotations

from urllib.parse import quote

from reasoning_engine.verifiable.sources import base
from reasoning_engine.verifiable.sources.result import RetrievalResult

ENDPOINT = "https://doaj.org/api/search/articles"


class DoajAdapter:
    name = "doaj"

    def search(self, run_id: str, query: str, limit: int = 10) -> RetrievalResult:
        url = f"{ENDPOINT}/{quote(query, safe='')}?pageSize={int(limit)}"
        try:
            payload = base.fetch_json(url)
        except base.AdapterError as exc:
            return RetrievalResult(evidence=[], error=exc.to_retrieval_error({"source": self.name}))

        results = payload.get("results", []) if isinstance(payload, dict) else []
        evidence = []
        for position, item in enumerate(results, start=1):
            bibjson = item.get("bibjson") or {}
            journal = bibjson.get("journal") or {}
            authors = [a.get("name", "") for a in bibjson.get("author", [])]
            doi = None
            url_link = None
            for identifier in bibjson.get("identifier", []):
                if identifier.get("type", "").lower() == "doi":
                    doi = identifier.get("id")
            for link in bibjson.get("link", []):
                if link.get("url"):
                    url_link = link.get("url")
                    break
            evidence.append(
                base.build_record(
                    run_id=run_id,
                    source_adapter=self.name,
                    query=query,
                    rank=position,
                    source_type="peer_reviewed_article",
                    title=bibjson.get("title") or "",
                    authors=[a for a in authors if a],
                    year=base.coerce_year(bibjson.get("year")),
                    publisher=journal.get("publisher", "") or "",
                    venue=journal.get("title", "") or "",
                    doi=doi,
                    url=url_link,
                    score=base.relevance_score(position, limit),
                    snippet=bibjson.get("abstract") or "",
                    licence_notes="Open access (DOAJ).",
                    metadata={"doaj_id": item.get("id")},
                )
            )
        return RetrievalResult(evidence=evidence)
