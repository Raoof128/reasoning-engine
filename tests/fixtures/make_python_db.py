"""Build a database with the Python implementation for Rust compatibility tests.

Usage: python3 -I tests/fixtures/make_python_db.py <db_path>

Writes the database through the real Python APIs, then prints a JSON
manifest to stdout: the IDs it created and every row of every table, in
rowid order. Only stdlib-based modules are imported, so no venv is needed.
"""

from __future__ import annotations

import json
import sqlite3
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "src"))

from reasoning_engine.db import init_db  # noqa: E402
from reasoning_engine.memory import record_reflection, save_memory  # noqa: E402
from reasoning_engine.sessions import (  # noqa: E402
    apply_planning_result,
    create_session,
    register_branch,
    score_branch,
)
from reasoning_engine.verifiable.claims import extract_claims, verify_claims  # noqa: E402
from reasoning_engine.verifiable.models import (  # noqa: E402
    EvidenceGap,
    EvidenceRecord,
    ResearchRun,
)
from reasoning_engine.verifiable.store import ResearchStore  # noqa: E402

TABLES = [
    "sessions",
    "branches",
    "reflections",
    "episodic_memory",
    "sources",
    "research_runs",
    "evidence_records",
    "evidence_gaps",
    "claims",
    "claim_verifications",
    "quality_gate_results",
    "provenance_events",
    "attestation_manifests",
]


def build(db_path: str) -> dict:
    db = init_db(db_path)

    session = create_session(db, "How do PRMs interact with MCTS for test-time compute scaling?")
    sid = session["id"]
    b1 = register_branch(
        db,
        sid,
        ["Step 1: café <b>bold</b>", "Step 2: نتیجه \U0001f600"],
        sources=[
            {
                "url": "https://example.org/a",
                "title": "Paper &amp; A",
                "excerpt": "PRMs score steps.",
                "relevance_score": 0.7,
            }
        ],
    )
    b2 = register_branch(db, sid, ["Path B"], parent_id=b1["id"], depth=1)
    b3 = register_branch(db, sid, ["Path C"])
    score_branch(db, b1["id"], 0.9, 0.4, "Strong <i>evidence</i>", 0.85, session_id=sid)
    score_branch(db, b2["id"], 0.3, 0.1, "Weak", 0.5)
    score_branch(db, b3["id"], 0.6, 0.2, "Okay", 0.6)
    reflection = record_reflection(
        db, b2["id"], sid, "Missing citations", "Added two sources", 0.3, 0.55
    )
    memory = save_memory(
        db,
        sid,
        "How do PRMs work?",
        ["PRMs score individual steps", "Dense supervision ‑ beats ORM"],
        ["prm", "reward-models"],
    )
    apply_planning_result(db, sid, [b2["id"]], [b3["id"]], 7)

    store = ResearchStore(db)
    run = ResearchRun(
        run_id="run_fixture0001",
        query="Retrieval augmented generation reduces hallucination",
        profile="general",
        mode="standard",
        created_at="2026-10-18T00:00:00Z",
    )
    store.save_run(run)
    store.append_provenance(run.run_id, "run_created", {"query": run.query})
    evidence = EvidenceRecord(
        evidence_id="ev_0001",
        run_id=run.run_id,
        source_adapter="openalex",
        source_type="peer_reviewed_article",
        title="Café ☕ résumé of RAG",
        authors=["A. Researcher", "رئوف"],
        year=2025,
        publisher="",
        venue="Journal of Retrieval",
        doi="10.1000/rag",
        url="https://doi.org/10.1000/rag",
        retrieved_at="2026-10-18T00:00:01Z",
        query=run.query,
        rank=1,
        score=0.82,
        snippet="Retrieval augmented generation reduces hallucination in language models.",
        licence_notes=None,
        risk_flags=[],
        metadata={"also_found_in": ["crossref"], "citations": 12, "ratio": 1e-05},
    )
    store.save_evidence(evidence)
    store.save_gap(
        EvidenceGap(
            gap_id="gap_fixture_0001",
            run_id=run.run_id,
            query="second source",
            reason="No second corroborating source found.",
            created_at="2026-10-18T00:00:02Z",
        )
    )
    claims = extract_claims(
        run.run_id,
        "Retrieval augmented generation reduces hallucination. It was 30% better.",
        run.profile,
    )
    verifications = verify_claims(claims, [evidence])
    for claim in claims:
        store.save_claim(claim)
    for verification in verifications:
        store.save_verification(run.run_id, verification)
    store.append_provenance(run.run_id, "scholar_search_completed", {"evidence_count": 1})

    conn = sqlite3.connect(db)
    conn.row_factory = sqlite3.Row
    rows = {
        table: [dict(row) for row in conn.execute(f"SELECT * FROM {table} ORDER BY rowid")]
        for table in TABLES
    }
    user_version = conn.execute("PRAGMA user_version").fetchone()[0]
    conn.close()

    return {
        "session_id": sid,
        "branch_ids": [b1["id"], b2["id"], b3["id"]],
        "reflection_id": reflection["id"],
        "memory_id": memory["id"],
        "run_id": run.run_id,
        "user_version": user_version,
        "rows": rows,
    }


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit("usage: make_python_db.py <db_path>")
    # ASCII-only output: piped stdout on Windows defaults to cp1252.
    print(json.dumps(build(sys.argv[1])))
