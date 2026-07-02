# Academic Source Aggregation — Design

Date: 2026-07-03
Status: Approved (design), pending implementation

## Problem

`verifiable/retrieval.py` ships only a mock `ScholarGatewayAdapter`: unless
`SCHOLAR_GATEWAY_LIVE=1` (plus a bearer token to a single connector URL), every
search returns fabricated fixtures. The verifiable pipeline therefore never
touches real literature. We want real, no-auth academic retrieval across all
fields, aggregated into the existing `EvidenceRecord` flow.

## Decisions (confirmed)

- **Sources:** OpenAlex, Crossref, arXiv, Semantic Scholar, Europe PMC, DOAJ,
  DBLP (all free / no key). Scholar Gateway kept as an optional adapter.
- **Combination:** aggregate across sources, dedup by DOI then title, unified
  deterministic ranking, one call = all sources.
- **Default mode:** live by default (no auth needed); `REASONING_ENGINE_OFFLINE=1`
  forces deterministic mock fixtures for tests/CI/no-network.
- **Concurrency:** `concurrent.futures.ThreadPoolExecutor` fan-out (I/O bound).
  Deterministic because dedup/rank run after all results are collected.
- **Dependencies:** stdlib (`urllib`, `concurrent.futures`) plus `defusedxml`
  for hardened arXiv Atom parsing (XXE / billion-laughs safe). arXiv is fetched
  over HTTPS. This adds one small, security-focused runtime dep alongside `mcp`.

## Module layout

```
verifiable/
  sources/
    __init__.py
    base.py             # SourceAdapter protocol, AdapterError, fetch_json/fetch_text, helpers
    mock.py             # MockScholarGatewayAdapter (moved), deterministic fixtures
    scholar_gateway.py  # ScholarGatewayAdapter (moved from retrieval.py)
    openalex.py
    crossref.py
    arxiv.py            # Atom XML via xml.etree
    semantic_scholar.py # optional S2_API_KEY
    europepmc.py
    doaj.py
    dblp.py
  aggregator.py         # AcademicSearchAggregator
  retrieval.py          # back-compat surface: re-exports + default_search_adapter()
```

`retrieval.py` re-exports `RetrievalResult`, `RetrievalError`,
`MockScholarGatewayAdapter`, `ScholarGatewayAdapter` so existing imports and
tests keep working. `service.py` changes one line: build
`default_search_adapter()` instead of `ScholarGatewayAdapter()`.

## Components

### base.py
- `SourceAdapter` protocol: `name: str`, `search(run_id, query, limit) -> RetrievalResult`.
- `AdapterError(error_type, message, retryable)` internal exception.
- `fetch_json(url, *, headers, timeout)` / `fetch_text(...)` — call
  `urllib.request.urlopen`; map `HTTPError` 401→`auth_required`,
  429→`rate_limited`, else→`unavailable`; `URLError`/`timeout`→`unavailable`;
  JSON decode failure→`malformed_response`. Called module-qualified
  (`base.fetch_json`) so tests can monkeypatch without network.
- `build_record(...)` helper that fills an `EvidenceRecord` with sane defaults.
- Query URLs are built with `urllib.parse.urlencode` / `quote`.
- A shared descriptive `User-Agent` header (some APIs require one; OpenAlex asks
  for a mailto in the UA — use `REASONING_ENGINE_CONTACT_EMAIL` if set).

### Each adapter
- Builds its endpoint URL, fetches, maps items → `EvidenceRecord` with the
  correct `source_type` (arXiv→`preprint`, DBLP→`conference_paper`/`article`,
  others→`peer_reviewed_article`), `source_adapter=<name>`, raw item in
  `metadata`. Provisional `evidence_id`/`rank`; the aggregator re-assigns final
  ones after ranking. On `AdapterError`, returns `RetrievalResult(evidence=[],
  error=RetrievalError(...))`.

### aggregator.py — `AcademicSearchAggregator`
- Constructor takes an explicit `adapters` list (injectable for tests) or, by
  default, `default_adapters()` reads `REASONING_ENGINE_SOURCES` (comma list) or
  enables all free sources; Scholar Gateway is included only when its token is
  configured.
- `search(run_id, query, limit)`:
  1. If `REASONING_ENGINE_OFFLINE=1` → delegate to `MockScholarGatewayAdapter`.
  2. Empty query → `RetrievalError("unsupported_query")` (matches current mock).
  3. Fan out to adapters via `ThreadPoolExecutor`, each fetching up to `limit`.
  4. Collect evidence + per-source errors.
  5. Dedup: normalized DOI (strip `https://doi.org/`, lowercase), else
     normalized title (alnum-lowercase). Keep the higher-scored duplicate; union
     `metadata["also_found_in"]`.
  6. Rank: sort by `score` desc, tie-break `(source_adapter, doi_or_title)` for
     determinism. `score` = blend of source relevance and log-scaled citation
     count, normalized to [0,1].
  7. Re-assign `evidence_id=f"ev_{i:04d}"` and `rank=i` post-sort (guarantees
     globally-unique PKs for the evidence table), truncate to `limit`.
  8. Return `RetrievalResult(evidence=...)`. If no evidence: return the
     representative error if every source failed, else empty (→ existing gap).

## Error handling

Uses the existing `RETRIEVAL_ERROR_TYPES` taxonomy. Partial failure is
tolerated: successful sources still contribute evidence; a top-level
`RetrievalError` is surfaced only when the aggregate yields nothing AND at least
one source failed. This preserves the current gap/attestation semantics in
`service.py` unchanged.

## Testing

- Per-adapter parser tests monkeypatch `base.fetch_json` / `base.fetch_text`
  with a captured fixture (committed under `tests/fixtures/`); assert normalized
  `EvidenceRecord` fields and `source_type`. No live network.
- Aggregator tests inject fake adapters to verify dedup (same DOI, same title),
  ranking order, evidence_id renumbering, limit truncation, all-fail →
  representative error, partial-fail → surviving evidence.
- Offline-mode test: `REASONING_ENGINE_OFFLINE=1` yields deterministic mock.
- All 119 existing tests stay green; `test_verifiable_retrieval.py` continues to
  pass against the re-exported adapters.

## Out of scope (YAGNI)

- Full-text/PDF fetching, citation-graph expansion, caching layer, rate-limit
  backoff/retry scheduling, ranking learned from feedback. Adapters return
  metadata + abstracts only.
