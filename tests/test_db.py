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
    assert version >= 2
