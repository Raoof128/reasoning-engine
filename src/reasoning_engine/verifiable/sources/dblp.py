"""DBLP adapter — computer-science bibliography, no auth."""

from __future__ import annotations

from urllib.parse import urlencode

from reasoning_engine.verifiable.sources import base
from reasoning_engine.verifiable.sources.result import RetrievalResult

ENDPOINT = "https://dblp.org/search/publ/api"

_TYPE_MAP = {
    "Conference and Workshop Papers": "conference_paper",
    "Journal Articles": "peer_reviewed_article",
    "Informal and Other Publications": "preprint",
}


def _authors(info: dict) -> list[str]:
    authors_field = info.get("authors") or {}
    author = authors_field.get("author")
    if isinstance(author, dict):
        author = [author]
    if not isinstance(author, list):
        return []
    names = []
    for entry in author:
        if isinstance(entry, dict):
            names.append(str(entry.get("text", "")).strip())
        elif entry:
            names.append(str(entry).strip())
    return [name for name in names if name]


class DblpAdapter:
    name = "dblp"

    def search(self, run_id: str, query: str, limit: int = 10) -> RetrievalResult:
        params = urlencode({"q": query, "format": "json", "h": limit})
        try:
            payload = base.fetch_json(f"{ENDPOINT}?{params}")
        except base.AdapterError as exc:
            return RetrievalResult(evidence=[], error=exc.to_retrieval_error({"source": self.name}))

        hits = (
            ((payload.get("result") or {}).get("hits") or {}).get("hit", [])
            if isinstance(payload, dict)
            else []
        )
        evidence = []
        for position, hit in enumerate(hits, start=1):
            info = hit.get("info") or {}
            evidence.append(
                base.build_record(
                    run_id=run_id,
                    source_adapter=self.name,
                    query=query,
                    rank=position,
                    source_type=_TYPE_MAP.get(info.get("type", ""), "peer_reviewed_article"),
                    title=info.get("title") or "",
                    authors=_authors(info),
                    year=base.coerce_year(info.get("year")),
                    venue=info.get("venue", "") or "",
                    doi=info.get("doi"),
                    url=info.get("ee") or info.get("url"),
                    score=base.relevance_score(position, limit),
                    snippet=info.get("title") or "",
                    metadata={"dblp_key": info.get("key"), "type": info.get("type")},
                )
            )
        return RetrievalResult(evidence=evidence)
