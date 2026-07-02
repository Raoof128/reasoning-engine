"""OpenAlex adapter — 250M+ works across every field, no auth."""

from __future__ import annotations

from urllib.parse import urlencode

from reasoning_engine.verifiable.sources import base
from reasoning_engine.verifiable.sources.result import RetrievalResult

ENDPOINT = "https://api.openalex.org/works"


def _reconstruct_abstract(inverted_index: dict | None) -> str:
    if not inverted_index:
        return ""
    positions: list[tuple[int, str]] = []
    for word, indexes in inverted_index.items():
        for idx in indexes:
            positions.append((idx, word))
    positions.sort(key=lambda pair: pair[0])
    return " ".join(word for _, word in positions)


class OpenAlexAdapter:
    name = "openalex"

    def search(self, run_id: str, query: str, limit: int = 10) -> RetrievalResult:
        params = urlencode({"search": query, "per_page": limit, "mailto": base._contact_email()})
        try:
            payload = base.fetch_json(f"{ENDPOINT}?{params}")
        except base.AdapterError as exc:
            return RetrievalResult(evidence=[], error=exc.to_retrieval_error({"source": self.name}))

        results = payload.get("results", []) if isinstance(payload, dict) else []
        evidence = []
        for position, item in enumerate(results, start=1):
            location = item.get("primary_location") or {}
            source = location.get("source") or {}
            authors = [
                (a.get("author") or {}).get("display_name", "") for a in item.get("authorships", [])
            ]
            evidence.append(
                base.build_record(
                    run_id=run_id,
                    source_adapter=self.name,
                    query=query,
                    rank=position,
                    source_type="peer_reviewed_article",
                    title=item.get("title") or item.get("display_name") or "",
                    authors=[a for a in authors if a],
                    year=base.coerce_year(item.get("publication_year")),
                    publisher=source.get("host_organization_name", "") or "",
                    venue=source.get("display_name", "") or "",
                    doi=item.get("doi"),
                    url=location.get("landing_page_url") or item.get("id"),
                    score=base.relevance_score(position, limit, item.get("cited_by_count")),
                    snippet=_reconstruct_abstract(item.get("abstract_inverted_index")),
                    metadata={
                        "openalex_id": item.get("id"),
                        "cited_by": item.get("cited_by_count"),
                    },
                )
            )
        return RetrievalResult(evidence=evidence)
