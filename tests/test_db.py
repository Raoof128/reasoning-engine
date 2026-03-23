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
