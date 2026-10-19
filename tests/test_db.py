import sqlite3

from reasoning_engine.db import init_db


def test_init_db_creates_tables(db_path):
    conn = sqlite3.connect(db_path)
    cursor = conn.execute("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
    tables = [row[0] for row in cursor.fetchall()]
    conn.close()

    assert "sessions" in tables
    assert "branches" in tables
    assert "reflections" in tables
    assert "episodic_memory" in tables
    assert "sources" in tables


def test_init_db_idempotent(db_path):
    """Calling init_db twice should not error."""
    init_db(db_path)
    conn = sqlite3.connect(db_path)
    cursor = conn.execute("SELECT name FROM sqlite_master WHERE type='table'")
    tables = [row[0] for row in cursor.fetchall()]
    conn.close()
    assert len(tables) >= 5


def test_init_db_creates_verifiable_research_tables(db_path):
    conn = sqlite3.connect(db_path)
    cursor = conn.execute("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
    tables = {row[0] for row in cursor.fetchall()}
    version = conn.execute("PRAGMA user_version").fetchone()[0]
    conn.close()

    assert "research_runs" in tables
    assert "evidence_records" in tables
    assert "evidence_gaps" in tables
    assert "claims" in tables
    assert "claim_verifications" in tables
    assert "quality_gate_results" in tables
    assert "provenance_events" in tables
    assert "attestation_manifests" in tables
    assert version >= 3


# The claim tables before version 3: keyed by the bare per-run IDs.
V2_CLAIM_TABLES = """
CREATE TABLE claims (
    claim_id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES research_runs(run_id),
    payload TEXT NOT NULL,
    status TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE TABLE claim_verifications (
    verification_id TEXT PRIMARY KEY,
    claim_id TEXT NOT NULL REFERENCES claims(claim_id),
    run_id TEXT NOT NULL REFERENCES research_runs(run_id),
    payload TEXT NOT NULL,
    support_status TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
"""


def _schema(path):
    conn = sqlite3.connect(path)
    rows = conn.execute(
        "SELECT type, name, tbl_name, COALESCE(sql, '') FROM sqlite_master ORDER BY type, name"
    ).fetchall()
    conn.close()
    return [(t, n, tbl, " ".join(sql.split())) for t, n, tbl, sql in rows]


def test_init_db_migrates_version_2_claim_tables(tmp_path):
    from reasoning_engine.db import SCHEMA_SQL
    from reasoning_engine.verifiable.models import ClaimRecord
    from reasoning_engine.verifiable.store import ResearchStore

    db = str(tmp_path / "v2.db")
    fresh = init_db(str(tmp_path / "fresh.db"))
    conn = sqlite3.connect(db)
    conn.executescript(V2_CLAIM_TABLES + SCHEMA_SQL)
    conn.executescript(
        """
        PRAGMA user_version = 2;
        INSERT INTO research_runs VALUES
            ('run_a', 'A', 'general', 'standard', 'complete', '2026-01-01T00:00:00Z'),
            ('run_b', 'B', 'general', 'standard', 'complete', '2026-01-02T00:00:00Z');
        -- run_b's claim_0001 already overwrote run_a's.
        INSERT INTO claims VALUES
            ('claim_0001', 'run_b', '{"text": "B1"}', 'supported', '2026-01-02 00:00:00'),
            ('claim_0002', 'run_a', '{"text": "A2"}', 'supported', '2026-01-01 00:00:00');
        INSERT INTO claim_verifications VALUES
            ('ver_a1', 'claim_0001', 'run_a', '{"v": "a1"}', 'supported', '2026-01-01 00:00:00'),
            ('ver_a2', 'claim_0002', 'run_a', '{"v": "a2"}', 'supported', '2026-01-01 00:00:00'),
            ('ver_b1', 'claim_0001', 'run_b', '{"v": "b1"}', 'supported', '2026-01-02 00:00:00');
        """
    )
    conn.close()

    init_db(db)

    assert _schema(db) == _schema(fresh)
    conn = sqlite3.connect(db)
    assert conn.execute("PRAGMA user_version").fetchone()[0] == 3
    created = conn.execute("SELECT created_at FROM claims WHERE run_id = 'run_a'").fetchone()[0]
    conn.close()
    assert created == "2026-01-01 00:00:00"
    store = ResearchStore(db)
    assert store.list_claims("run_a") == [{"text": "A2"}]
    assert store.list_claims("run_b") == [{"text": "B1"}]
    assert len(store.list_verifications("run_a")) == 2
    assert len(store.list_verifications("run_b")) == 1

    # From now on run_a can store its claim_0001 beside run_b's.
    store.save_claim(
        ClaimRecord(
            claim_id="claim_0001",
            run_id="run_a",
            text="A1",
            claim_type="empirical",
            domain="general",
            importance="high",
            risk_level="low",
            requires_citation=True,
            created_from="test",
        )
    )
    assert len(store.list_claims("run_a")) == 2
    assert store.list_claims("run_b") == [{"text": "B1"}]

    schema = _schema(db)
    init_db(db)
    assert _schema(db) == schema
