"""arXiv adapter — preprints across physics, CS, math, etc. Atom XML, no auth."""

from __future__ import annotations

import re
import string
from urllib.parse import urlencode

# defusedxml hardens parsing against XXE / billion-laughs; the arXiv feed is
# fetched over the network, so it is untrusted input.
from defusedxml.ElementTree import ParseError, fromstring

from reasoning_engine.verifiable.sources import base
from reasoning_engine.verifiable.sources.result import RetrievalResult

ENDPOINT = "https://export.arxiv.org/api/query"
_ATOM = "{http://www.w3.org/2005/Atom}"
_ARXIV = "{http://arxiv.org/schemas/atom}"

# Function words that occur in nearly every paper. Kept ASCII, like the rest
# of the tokenising below, so the Rust port splits queries identically.
_STOPWORDS = frozenset(
    {
        "a", "an", "and", "are", "as", "at", "be", "been", "but", "by", "can", "could", "did",
        "do", "does", "for", "from", "has", "have", "how", "if", "in", "into", "is", "it", "its",
        "no", "not", "of", "on", "or", "than", "that", "the", "their", "there", "these", "this",
        "those", "to", "versus", "vs", "was", "were", "what", "when", "where", "which", "who",
        "whom", "why", "will", "with", "would",
    }
)  # fmt: skip
_SPACE = re.compile(r"[ \t\n\r\x0b\x0c]+")


def search_query(query: str) -> str:
    """The arXiv `search_query` for a free-text query.

    arXiv binds a field prefix such as `all:` to the first word only and ORs
    the rest across every field, so `all:{query}` matched "Does", "in" or "2"
    anywhere in the archive. Each content word gets its own `all:` instead;
    arXiv ranks papers that match more of them first. Requiring every word
    (AND) finds nothing for most full-sentence questions.
    """
    terms = []
    for token in _SPACE.split(query):
        term = token.strip(string.punctuation)
        if len(term) < 2 or all("0" <= c <= "9" for c in term):
            continue
        if term.lower() not in _STOPWORDS:
            terms.append(term)
    if not terms:
        return f"all:{query}"
    return " OR ".join(f"all:{term}" for term in terms)


class ArxivAdapter:
    name = "arxiv"

    def search(self, run_id: str, query: str, limit: int = 10) -> RetrievalResult:
        params = urlencode(
            {
                "search_query": search_query(query),
                "start": 0,
                "max_results": limit,
                "sortBy": "relevance",
            }
        )
        try:
            body = base.fetch_text(f"{ENDPOINT}?{params}")
        except base.AdapterError as exc:
            return RetrievalResult(evidence=[], error=exc.to_retrieval_error({"source": self.name}))

        try:
            root = fromstring(body)
        except (ParseError, ValueError) as exc:
            # ValueError covers defusedxml's EntitiesForbidden / DTDForbidden guards.
            return RetrievalResult(
                evidence=[],
                error=base.AdapterError(
                    "malformed_response", f"invalid Atom feed: {exc}", retryable=False
                ).to_retrieval_error({"source": self.name}),
            )

        evidence = []
        for position, entry in enumerate(root.findall(f"{_ATOM}entry"), start=1):
            title = (entry.findtext(f"{_ATOM}title") or "").strip()
            summary = (entry.findtext(f"{_ATOM}summary") or "").strip()
            published = entry.findtext(f"{_ATOM}published") or ""
            landing = entry.findtext(f"{_ATOM}id") or ""
            doi = entry.findtext(f"{_ARXIV}doi")
            authors = [
                (author.findtext(f"{_ATOM}name") or "").strip()
                for author in entry.findall(f"{_ATOM}author")
            ]
            evidence.append(
                base.build_record(
                    run_id=run_id,
                    source_adapter=self.name,
                    query=query,
                    rank=position,
                    source_type="preprint",
                    title=title,
                    authors=[a for a in authors if a],
                    year=base.coerce_year(published),
                    publisher="arXiv",
                    venue="arXiv",
                    doi=doi,
                    url=landing or None,
                    score=base.relevance_score(position, limit),
                    snippet=summary,
                    licence_notes="arXiv preprint (not peer-reviewed).",
                    metadata={"arxiv_id": landing},
                )
            )
        return RetrievalResult(evidence=evidence)
