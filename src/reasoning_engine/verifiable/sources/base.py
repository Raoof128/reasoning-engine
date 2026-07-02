"""Shared plumbing for academic source adapters.

Every adapter is a small, self-contained unit that turns one provider's search
API into a list of normalized ``EvidenceRecord`` objects. The HTTP helpers here
are called module-qualified (``base.fetch_json(...)``) so tests can monkeypatch
them without touching the network.
"""

from __future__ import annotations

import json
import math
import os
import re
import urllib.error
import urllib.request
from typing import Any, Protocol

from reasoning_engine.verifiable.models import EvidenceRecord, RetrievalError, utc_now

DEFAULT_TIMEOUT = 25
_TAG_RE = re.compile(r"<[^>]+>")


def strip_tags(text: str) -> str:
    return _TAG_RE.sub(" ", text or "")


def relevance_score(position: int, limit: int, citations: int | None = None) -> float:
    """Blend within-source rank position with log-scaled citation count into [0, 1]."""
    pos_component = max(0.0, 1.0 - (position - 1) / max(limit, 1))
    if citations and citations > 0:
        cite_component = min(1.0, math.log1p(citations) / math.log(1000))
    else:
        cite_component = 0.0
    return round(0.6 * pos_component + 0.4 * cite_component, 4)


def _contact_email() -> str:
    return os.environ.get("REASONING_ENGINE_CONTACT_EMAIL", "reasoning-engine@example.org")


def user_agent() -> str:
    # Several providers (notably OpenAlex/Crossref) ask for a descriptive
    # User-Agent with a contact address for their "polite pool".
    return f"reasoning-engine/0.1 (mailto:{_contact_email()})"


class AdapterError(Exception):
    """Internal error raised by fetch helpers, mapped to RetrievalError by adapters."""

    def __init__(self, error_type: str, message: str, retryable: bool) -> None:
        super().__init__(message)
        self.error_type = error_type
        self.message = message
        self.retryable = retryable

    def to_retrieval_error(self, metadata: dict[str, Any] | None = None) -> RetrievalError:
        return RetrievalError(
            error_type=self.error_type,
            message=self.message,
            retryable=self.retryable,
            metadata=metadata or {},
        )


class SourceAdapter(Protocol):
    name: str

    def search(self, run_id: str, query: str, limit: int = 10):  # -> RetrievalResult
        raise NotImplementedError


def _map_http_error(exc: urllib.error.HTTPError) -> AdapterError:
    if exc.code == 401:
        return AdapterError("auth_required", "HTTP 401 from provider", retryable=False)
    if exc.code == 429:
        return AdapterError("rate_limited", "HTTP 429 from provider", retryable=True)
    return AdapterError("unavailable", f"HTTP {exc.code} from provider", retryable=True)


def _request(url: str, headers: dict[str, str] | None, timeout: int) -> bytes:
    merged = {"User-Agent": user_agent(), "Accept": "application/json"}
    if headers:
        merged.update(headers)
    request = urllib.request.Request(url, headers=merged, method="GET")
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            return response.read()
    except urllib.error.HTTPError as exc:
        raise _map_http_error(exc) from exc
    except (urllib.error.URLError, TimeoutError, OSError) as exc:
        raise AdapterError("unavailable", str(exc), retryable=True) from exc


def fetch_json(
    url: str, *, headers: dict[str, str] | None = None, timeout: int = DEFAULT_TIMEOUT
) -> Any:
    raw = _request(url, headers, timeout)
    try:
        return json.loads(raw.decode("utf-8"))
    except (json.JSONDecodeError, UnicodeDecodeError) as exc:
        raise AdapterError(
            "malformed_response", f"invalid JSON body: {exc}", retryable=False
        ) from exc


def fetch_text(
    url: str, *, headers: dict[str, str] | None = None, timeout: int = DEFAULT_TIMEOUT
) -> str:
    raw = _request(url, headers, timeout)
    try:
        return raw.decode("utf-8")
    except UnicodeDecodeError as exc:
        raise AdapterError(
            "malformed_response", f"invalid text body: {exc}", retryable=False
        ) from exc


def coerce_year(value: Any) -> int | None:
    """Best-effort year extraction from ints, strings, or ISO date prefixes."""
    if value is None:
        return None
    if isinstance(value, bool):
        return None
    if isinstance(value, int):
        return value if 1000 <= value <= 2200 else None
    text = str(value).strip()
    if len(text) >= 4 and text[:4].isdigit():
        year = int(text[:4])
        return year if 1000 <= year <= 2200 else None
    return None


def clip_snippet(text: str, limit: int = 1500) -> str:
    cleaned = " ".join((text or "").split())
    return cleaned[:limit]


def build_record(
    *,
    run_id: str,
    source_adapter: str,
    query: str,
    rank: int,
    source_type: str = "peer_reviewed_article",
    title: str = "",
    authors: list[str] | None = None,
    year: int | None = None,
    publisher: str = "",
    venue: str = "",
    doi: str | None = None,
    url: str | None = None,
    score: float = 0.0,
    snippet: str = "",
    licence_notes: str | None = None,
    metadata: dict[str, Any] | None = None,
) -> EvidenceRecord:
    """Fill an EvidenceRecord with normalized values and sane defaults."""
    return EvidenceRecord(
        evidence_id=f"ev_{source_adapter}_{rank:04d}",
        run_id=run_id,
        source_adapter=source_adapter,
        source_type=source_type,
        title=clip_snippet(title, 500) or "Untitled",
        authors=authors or [],
        year=year,
        publisher=publisher or "",
        venue=venue or "",
        doi=normalize_doi(doi),
        url=url,
        retrieved_at=utc_now(),
        query=query,
        rank=rank,
        score=float(score),
        snippet=clip_snippet(snippet),
        licence_notes=licence_notes,
        risk_flags=[],
        metadata=metadata or {},
    )


def normalize_doi(doi: str | None) -> str | None:
    if not doi:
        return None
    text = str(doi).strip().lower()
    for prefix in ("https://doi.org/", "http://doi.org/", "doi:"):
        if text.startswith(prefix):
            text = text[len(prefix) :]
    return text or None
