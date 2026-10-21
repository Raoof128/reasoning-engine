import sqlite3

import pytest

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
    assert version >= 4


# evidence_records before version 4: keyed by the bare per-run evidence ID.
V3_EVIDENCE_TABLE = """
CREATE TABLE evidence_records (
    evidence_id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES research_runs(run_id),
    payload TEXT NOT NULL,
    snippet_hash TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
"""

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


def _write_legacy_db(db, legacy_tables, version):
    """An older database where run_b's ev_0001 and claim_0001 already overwrote run_a's."""
    from reasoning_engine.db import SCHEMA_SQL

    conn = sqlite3.connect(db)
    # Both implementations have always switched the file to WAL on open.
    conn.execute("PRAGMA journal_mode = WAL")
    conn.executescript(legacy_tables + SCHEMA_SQL)
    conn.execute(f"PRAGMA user_version = {version}")
    conn.executescript(
        """
        INSERT INTO research_runs VALUES
            ('run_a', 'A', 'general', 'standard', 'complete', '2026-01-01T00:00:00Z'),
            ('run_b', 'B', 'general', 'standard', 'complete', '2026-01-02T00:00:00Z');
        INSERT INTO evidence_records VALUES
            ('ev_0001', 'run_b', '{"snippet": "B1"}', 'sha256:b1', '2026-01-02 00:00:00'),
            ('ev_0002', 'run_a', '{"snippet": "A2"}', 'sha256:a2', '2026-01-01 00:00:00');
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


@pytest.mark.parametrize(
    ("legacy_tables", "version"),
    [(V3_EVIDENCE_TABLE + V2_CLAIM_TABLES, 2), (V3_EVIDENCE_TABLE, 3)],
    ids=["v2", "v3"],
)
def test_init_db_migrates_older_run_keyed_tables(tmp_path, legacy_tables, version):
    from reasoning_engine.verifiable.models import ClaimRecord
    from reasoning_engine.verifiable.store import ResearchStore

    db = str(tmp_path / "legacy.db")
    fresh = init_db(str(tmp_path / "fresh.db"))
    _write_legacy_db(db, legacy_tables, version)

    init_db(db)

    assert _schema(db) == _schema(fresh)
    conn = sqlite3.connect(db)
    assert conn.execute("PRAGMA user_version").fetchone()[0] == 4
    evidence_row = conn.execute(
        "SELECT snippet_hash, created_at FROM evidence_records WHERE run_id = 'run_a'"
    ).fetchone()
    claim_created = conn.execute("SELECT created_at FROM claims WHERE run_id = 'run_a'").fetchone()[
        0
    ]
    conn.close()
    assert evidence_row == ("sha256:a2", "2026-01-01 00:00:00")
    assert claim_created == "2026-01-01 00:00:00"
    store = ResearchStore(db)
    assert store.list_evidence("run_a") == [{"snippet": "A2"}]
    assert store.list_evidence("run_b") == [{"snippet": "B1"}]
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


def test_concurrent_init_db_migrates_a_legacy_database_once(tmp_path):
    import threading
    from concurrent.futures import ThreadPoolExecutor

    db = str(tmp_path / "legacy.db")
    fresh = init_db(str(tmp_path / "fresh.db"))
    _write_legacy_db(db, V3_EVIDENCE_TABLE + V2_CLAIM_TABLES, 2)

    # Every opener may see the legacy schema before any of them migrates; the
    # losers wait on the write lock and rebuild an already-run-keyed schema,
    # which must be a no-op on the data.
    openers = 4
    barrier = threading.Barrier(openers)

    def open_db(_):
        barrier.wait()
        return init_db(db)

    with ThreadPoolExecutor(openers) as pool:
        list(pool.map(open_db, range(openers)))

    assert _schema(db) == _schema(fresh)
    conn = sqlite3.connect(db)
    counts = [
        conn.execute(f"SELECT COUNT(*) FROM {table}").fetchone()[0]
        for table in ("evidence_records", "claims", "claim_verifications")
    ]
    version = conn.execute("PRAGMA user_version").fetchone()[0]
    conn.close()
    assert counts == [2, 2, 3]
    assert version == 4
