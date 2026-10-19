# Recorded academic API responses

Fixtures for the Rust `re-retrieval` parity tests. Do not hand-edit
`expected.json`; regenerate it.

| File | What it is |
|---|---|
| `<source>.body` | A live response body for the query in `recorded.json` (public metadata; recorded with `reasoning-engine@example.org`, no API keys). |
| `<source>__<case>.body` | Hand-written edge cases (`EDGE_CASES` in `record_http.py`) and captured oddities such as DBLP's anti-bot page (`dblp__bot_challenge`). |
| `recorded.json` | Query and limit used for each live recording. |
| `expected.json` | What the Python adapters return for every body (`cases`), the URL they request for awkward queries (`urls`), and what the Python aggregator makes of the combined results (`aggregates`). `retrieved_at` is stripped. |

Regenerate, from the repo root:

```bash
python3 -I tests/fixtures/record_http.py record    # network: refresh the live bodies
.venv/bin/python -I tests/fixtures/record_http.py expected  # offline: rebuild expected.json
```

DBLP serves a 202 anti-bot page to datacenter IPs and Semantic Scholar may
answer 429; `record` skips a source it cannot record and keeps the old body.
