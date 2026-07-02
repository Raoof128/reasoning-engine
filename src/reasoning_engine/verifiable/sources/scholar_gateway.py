"""Scholar Gateway MCP connector — optional, token-gated adapter."""

from __future__ import annotations

import json
import os
import urllib.error
import urllib.request

from reasoning_engine.verifiable.models import EvidenceRecord, RetrievalError, utc_now
from reasoning_engine.verifiable.sources.mock import MockScholarGatewayAdapter
from reasoning_engine.verifiable.sources.result import RetrievalResult

SCHOLAR_GATEWAY_MCP_URL = "https://connector.scholargateway.ai/mcp"


class ScholarGatewayAdapter:
    name = "scholar_gateway"

    def __init__(self, endpoint: str = SCHOLAR_GATEWAY_MCP_URL):
        self.endpoint = endpoint
        self.mock_adapter = MockScholarGatewayAdapter()

    @staticmethod
    def is_configured() -> bool:
        return os.environ.get("SCHOLAR_GATEWAY_LIVE") == "1" and bool(
            os.environ.get("SCHOLAR_GATEWAY_ACCESS_TOKEN")
        )

    def search(self, run_id: str, query: str, limit: int = 10) -> RetrievalResult:
        if os.environ.get("SCHOLAR_GATEWAY_LIVE") != "1":
            return self.mock_adapter.search(run_id, query, limit)

        token = os.environ.get("SCHOLAR_GATEWAY_ACCESS_TOKEN")
        if not token:
            return RetrievalResult(
                evidence=[],
                error=RetrievalError(
                    "auth_required",
                    "Set SCHOLAR_GATEWAY_ACCESS_TOKEN or complete Scholar Gateway OAuth setup.",
                    retryable=False,
                    metadata={"env_var": "SCHOLAR_GATEWAY_ACCESS_TOKEN"},
                ),
            )

        payload = {
            "jsonrpc": "2.0",
            "id": "semantic_search",
            "method": "tools/call",
            "params": {
                "name": "semantic_search",
                "arguments": {"query": query, "limit": limit},
            },
        }
        request = urllib.request.Request(
            self.endpoint,
            data=json.dumps(payload).encode(),
            headers={
                "Authorization": f"Bearer {token}",
                "Content-Type": "application/json",
                "Accept": "application/json",
            },
            method="POST",
        )
        try:
            with urllib.request.urlopen(request, timeout=30) as response:
                raw = json.loads(response.read().decode())
        except urllib.error.HTTPError as exc:
            if exc.code == 401:
                error_type = "auth_required"
                retryable = False
            elif exc.code == 429:
                error_type = "rate_limited"
                retryable = True
            else:
                error_type = "unavailable"
                retryable = True
            return RetrievalResult(
                evidence=[],
                error=RetrievalError(error_type, f"Scholar Gateway HTTP {exc.code}", retryable, {}),
            )
        except (OSError, TimeoutError, json.JSONDecodeError) as exc:
            return RetrievalResult(
                evidence=[],
                error=RetrievalError("unavailable", str(exc), retryable=True, metadata={}),
            )

        try:
            items = raw.get("result", {}).get("content", [])
            evidence = [
                EvidenceRecord(
                    evidence_id=f"ev_{index:04d}",
                    run_id=run_id,
                    source_adapter="scholar_gateway",
                    source_type="peer_reviewed_article",
                    title=str(item.get("title", "Untitled")),
                    authors=list(item.get("authors", [])),
                    year=item.get("year"),
                    publisher=str(item.get("publisher", "Wiley")),
                    venue=str(item.get("venue", "")),
                    doi=item.get("doi"),
                    url=item.get("url"),
                    retrieved_at=utc_now(),
                    query=query,
                    rank=index,
                    score=float(item.get("score", 0.0)),
                    snippet=str(item.get("snippet", "")),
                    licence_notes=item.get("licence_notes"),
                    risk_flags=[],
                    metadata={"raw": item},
                )
                for index, item in enumerate(items, start=1)
            ]
        except (TypeError, ValueError) as exc:
            return RetrievalResult(
                evidence=[],
                error=RetrievalError("malformed_response", str(exc), retryable=False, metadata={}),
            )

        if not evidence:
            return RetrievalResult(
                evidence=[],
                error=RetrievalError(
                    "empty_result", "Scholar Gateway returned no results.", False, {}
                ),
            )
        return RetrievalResult(evidence=evidence)
