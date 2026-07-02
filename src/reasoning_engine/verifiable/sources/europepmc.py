"""Europe PMC adapter — biomedical and life-sciences literature, no auth."""

from __future__ import annotations

from urllib.parse import urlencode

from reasoning_engine.verifiable.sources import base
from reasoning_engine.verifiable.sources.result import RetrievalResult

ENDPOINT = "https://www.ebi.ac.uk/europepmc/webservices/rest/search"


class EuropePmcAdapter:
    name = "europepmc"

    def search(self, run_id: str, query: str, limit: int = 10) -> RetrievalResult:
        params = urlencode(
            {"query": query, "format": "json", "pageSize": limit, "resultType": "core"}
        )
        try:
            payload = base.fetch_json(f"{ENDPOINT}?{params}")
        except base.AdapterError as exc:
            return RetrievalResult(evidence=[], error=exc.to_retrieval_error({"source": self.name}))

        results = (
            (payload.get("resultList") or {}).get("result", []) if isinstance(payload, dict) else []
        )
        evidence = []
        for position, item in enumerate(results, start=1):
            journal = ((item.get("journalInfo") or {}).get("journal") or {}).get("title", "")
            author_string = item.get("authorString", "") or ""
            authors = [a.strip() for a in author_string.rstrip(".").split(",") if a.strip()]
            evidence.append(
                base.build_record(
                    run_id=run_id,
                    source_adapter=self.name,
                    query=query,
                    rank=position,
                    source_type="peer_reviewed_article",
                    title=item.get("title") or "",
                    authors=authors,
                    year=base.coerce_year(item.get("pubYear")),
                    venue=journal or "",
                    doi=item.get("doi"),
                    url=(
                        f"https://europepmc.org/abstract/{item.get('source')}/{item.get('id')}"
                        if item.get("source") and item.get("id")
                        else None
                    ),
                    score=base.relevance_score(position, limit, item.get("citedByCount")),
                    snippet=item.get("abstractText") or "",
                    metadata={"cited_by": item.get("citedByCount"), "pmid": item.get("pmid")},
                )
            )
        return RetrievalResult(evidence=evidence)
