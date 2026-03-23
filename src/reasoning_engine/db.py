"""SQLite database schema and initialization for the reasoning engine."""

import sqlite3
from contextlib import contextmanager
from pathlib import Path

DEFAULT_DB_PATH = Path(__file__).parent / "reasoning.db"


def init_db(db_path: str | None = None) -> str:
    """Create all tables. Idempotent — safe to call multiple times."""
    path = db_path or str(DEFAULT_DB_PATH)
    conn = sqlite3.connect(path)
    try:
        conn.executescript("""
            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                query TEXT NOT NULL,
                difficulty REAL NOT NULL DEFAULT 0.5,
                strategy TEXT NOT NULL DEFAULT 'best_of_n',
                budget_total_branches INTEGER NOT NULL DEFAULT 3,
                budget_max_depth INTEGER NOT NULL DEFAULT 5,
                budget_max_steps INTEGER NOT NULL DEFAULT 15,
                budget_tokens_per_branch INTEGER NOT NULL DEFAULT 4000,
                budget_remaining_steps INTEGER NOT NULL DEFAULT 15,
                status TEXT NOT NULL DEFAULT 'active',
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS branches (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL REFERENCES sessions(id),
                parent_id TEXT,
                depth INTEGER NOT NULL DEFAULT 0,
                trace TEXT NOT NULL DEFAULT '[]',
                q_score REAL NOT NULL DEFAULT 0.0,
                advantage REAL NOT NULL DEFAULT 0.0,
                critique TEXT NOT NULL DEFAULT '',
                confidence REAL NOT NULL DEFAULT 0.0,
                visits INTEGER NOT NULL DEFAULT 0,
                status TEXT NOT NULL DEFAULT 'active',
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS reflections (
                id TEXT PRIMARY KEY,
                branch_id TEXT NOT NULL REFERENCES branches(id),
                session_id TEXT NOT NULL REFERENCES sessions(id),
                original_critique TEXT NOT NULL,
                revision_summary TEXT NOT NULL,
                score_before REAL NOT NULL DEFAULT 0.0,
                score_after REAL NOT NULL DEFAULT 0.0,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS episodic_memory (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL REFERENCES sessions(id),
                query TEXT NOT NULL,
                key_learnings TEXT NOT NULL DEFAULT '[]',
                domain_tags TEXT NOT NULL DEFAULT '[]',
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS sources (
                id TEXT PRIMARY KEY,
                branch_id TEXT NOT NULL REFERENCES branches(id),
                url TEXT NOT NULL,
                title TEXT NOT NULL DEFAULT '',
                excerpt TEXT NOT NULL DEFAULT '',
                relevance_score REAL NOT NULL DEFAULT 0.0,
                crawled_at TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE INDEX IF NOT EXISTS idx_branches_session ON branches(session_id);
            CREATE INDEX IF NOT EXISTS idx_branches_status ON branches(session_id, status);
            CREATE INDEX IF NOT EXISTS idx_reflections_session ON reflections(session_id);
            CREATE INDEX IF NOT EXISTS idx_sources_branch ON sources(branch_id);
            CREATE INDEX IF NOT EXISTS idx_memory_query ON episodic_memory(query);
        """)
    finally:
        conn.close()
    return path


def get_conn(db_path: str | None = None) -> sqlite3.Connection:
    """Get a connection with row_factory set to sqlite3.Row."""
    path = db_path or str(DEFAULT_DB_PATH)
    conn = sqlite3.connect(path)
    conn.row_factory = sqlite3.Row
    conn.execute("PRAGMA foreign_keys = ON")
    return conn


@contextmanager
def get_connection(db_path: str | None = None):
    """Context manager for database connections. Ensures cleanup on error."""
    conn = get_conn(db_path)
    try:
        yield conn
        conn.commit()
    except Exception:
        conn.rollback()
        raise
    finally:
        conn.close()
