"""Crossref adapter — 150M+ DOI-registered works, all fields, no auth."""

from __future__ import annotations

from urllib.parse import urlencode

from reasoning_engine.verifiable.sources import base
from reasoning_engine.verifiable.sources.result import RetrievalResult

ENDPOINT = "https://api.crossref.org/works"


def _first(seq: list | None) -> str:
    if isinstance(seq, list) and seq:
        return str(seq[0])
    return ""


def _year(item: dict) -> int | None:
    for key in ("published", "published-print", "published-online", "issued"):
        parts = (item.get(key) or {}).get("date-parts")
        if isinstance(parts, list) and parts and isinstance(parts[0], list) and parts[0]:
            return base.coerce_year(parts[0][0])
    return None


class CrossrefAdapter:
    name = "crossref"

    def search(self, run_id: str, query: str, limit: int = 10) -> RetrievalResult:
        params = urlencode(
            {
                "query": query,
                "rows": limit,
                "mailto": base._contact_email(),
                "select": "title,author,DOI,publisher,container-title,abstract,is-referenced-by-count,URL,issued,published",
            }
        )
        try:
            payload = base.fetch_json(f"{ENDPOINT}?{params}")
        except base.AdapterError as exc:
            return RetrievalResult(evidence=[], error=exc.to_retrieval_error({"source": self.name}))

        items = (payload.get("message") or {}).get("items", []) if isinstance(payload, dict) else []
        evidence = []
        for position, item in enumerate(items, start=1):
            authors = [
                " ".join(part for part in (a.get("given"), a.get("family")) if part)
                for a in item.get("author", [])
            ]
            evidence.append(
                base.build_record(
                    run_id=run_id,
                    source_adapter=self.name,
                    query=query,
                    rank=position,
                    source_type="peer_reviewed_article",
                    title=_first(item.get("title")),
                    authors=[a for a in authors if a],
                    year=_year(item),
                    publisher=item.get("publisher", "") or "",
                    venue=_first(item.get("container-title")),
                    doi=item.get("DOI"),
                    url=item.get("URL"),
                    score=base.relevance_score(position, limit, item.get("is-referenced-by-count")),
                    snippet=base.strip_tags(item.get("abstract", "")),
                    metadata={"cited_by": item.get("is-referenced-by-count")},
                )
            )
        return RetrievalResult(evidence=evidence)
