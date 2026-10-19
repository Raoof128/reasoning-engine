"""SQLite database schema and initialization for the reasoning engine."""

import sqlite3
from contextlib import contextmanager
from pathlib import Path

DEFAULT_DB_PATH = Path(__file__).parent / "reasoning.db"
DB_FILE_MODE = 0o600
# Version 3 keys claims and claim_verifications by run; see _migrate_claims.
USER_VERSION = 3

SCHEMA_SQL = """
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

CREATE TABLE IF NOT EXISTS research_runs (
    run_id TEXT PRIMARY KEY,
    query TEXT NOT NULL,
    profile TEXT NOT NULL,
    mode TEXT NOT NULL,
    status TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS evidence_records (
    evidence_id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES research_runs(run_id),
    payload TEXT NOT NULL,
    snippet_hash TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS evidence_gaps (
    gap_id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES research_runs(run_id),
    payload TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS claims (
    claim_id TEXT NOT NULL,
    run_id TEXT NOT NULL REFERENCES research_runs(run_id),
    payload TEXT NOT NULL,
    status TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (run_id, claim_id)
);

CREATE TABLE IF NOT EXISTS claim_verifications (
    verification_id TEXT NOT NULL,
    claim_id TEXT NOT NULL,
    run_id TEXT NOT NULL REFERENCES research_runs(run_id),
    payload TEXT NOT NULL,
    support_status TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (run_id, verification_id),
    FOREIGN KEY (run_id, claim_id) REFERENCES claims(run_id, claim_id)
);

CREATE TABLE IF NOT EXISTS quality_gate_results (
    gate_id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES research_runs(run_id),
    payload TEXT NOT NULL,
    result TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS provenance_events (
    event_id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES research_runs(run_id),
    event_type TEXT NOT NULL,
    payload TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS attestation_manifests (
    run_id TEXT PRIMARY KEY REFERENCES research_runs(run_id),
    payload TEXT NOT NULL,
    run_pack_hash TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_evidence_run ON evidence_records(run_id);
CREATE INDEX IF NOT EXISTS idx_gaps_run ON evidence_gaps(run_id);
CREATE INDEX IF NOT EXISTS idx_claims_run ON claims(run_id);
CREATE INDEX IF NOT EXISTS idx_verifications_run ON claim_verifications(run_id);
CREATE INDEX IF NOT EXISTS idx_provenance_run ON provenance_events(run_id);
"""

# Primary-key columns of claims plus claim_verifications: 4 once both are
# keyed by (run_id, ...), 2 for the version-2 tables keyed by the bare claim
# and verification IDs. Claim IDs restart at claim_0001 in every run, so under
# version 2 a second run overwrote the first run's claims. The check reads the
# schema, not user_version, because older builds reset the version to 2.
_CLAIMS_V2_CHECK = """
SELECT
    (SELECT COUNT(*) FROM pragma_table_info('claims') WHERE pk > 0)
  + (SELECT COUNT(*) FROM pragma_table_info('claim_verifications') WHERE pk > 0)
"""

# Move the version-2 tables aside so SCHEMA_SQL can create the run-keyed ones
# under the same names.
_CLAIMS_V2_SET_ASIDE = """
DROP INDEX IF EXISTS idx_claims_run;
DROP INDEX IF EXISTS idx_verifications_run;
ALTER TABLE claim_verifications RENAME TO claim_verifications_v2;
ALTER TABLE claims RENAME TO claims_v2;
"""

# Copy every row into the run-keyed tables, then drop the old ones.
# Verifications whose claim a later run already overwrote are kept; they stay
# readable by run even though their claim row is gone.
_CLAIMS_V2_COPY = """
INSERT INTO claims (claim_id, run_id, payload, status, created_at)
    SELECT claim_id, run_id, payload, status, created_at FROM claims_v2;
INSERT INTO claim_verifications
    (verification_id, claim_id, run_id, payload, support_status, created_at)
    SELECT verification_id, claim_id, run_id, payload, support_status, created_at
    FROM claim_verifications_v2;
DROP TABLE claim_verifications_v2;
DROP TABLE claims_v2;
"""


def _prepare_db_path(path: str) -> Path:
    db_path = Path(path).expanduser()
    db_path.parent.mkdir(parents=True, exist_ok=True)
    return db_path


def _harden_connection(conn: sqlite3.Connection) -> None:
    conn.execute("PRAGMA foreign_keys = ON")
    conn.execute("PRAGMA busy_timeout = 5000")
    conn.execute("PRAGMA journal_mode = WAL")
    conn.execute("PRAGMA synchronous = NORMAL")


def _migrate_claims(conn: sqlite3.Connection) -> None:
    """Re-key version-2 claims and claim_verifications by run, keeping every row.

    Foreign keys are off for the copy because a version-2 file can hold
    verifications whose claim a later run overwrote.
    """
    if conn.execute(_CLAIMS_V2_CHECK).fetchone()[0] == 4:
        return
    conn.execute("PRAGMA foreign_keys = OFF")
    try:
        conn.executescript(f"BEGIN;{_CLAIMS_V2_SET_ASIDE}{SCHEMA_SQL}{_CLAIMS_V2_COPY}COMMIT;")
    except Exception:
        conn.rollback()
        raise
    finally:
        conn.execute("PRAGMA foreign_keys = ON")


def init_db(db_path: str | None = None) -> str:
    """Create all tables. Idempotent — safe to call multiple times."""
    path = _prepare_db_path(db_path or str(DEFAULT_DB_PATH))
    existed = path.exists()
    conn = sqlite3.connect(path)
    try:
        _harden_connection(conn)
        conn.executescript(SCHEMA_SQL)
        _migrate_claims(conn)
        conn.execute(f"PRAGMA user_version = {USER_VERSION}")
        conn.commit()
    finally:
        conn.close()
    if not existed:
        path.chmod(DB_FILE_MODE)
    return str(path)


def get_conn(db_path: str | None = None) -> sqlite3.Connection:
    """Get a connection with row_factory set to sqlite3.Row."""
    path = _prepare_db_path(db_path or str(DEFAULT_DB_PATH))
    conn = sqlite3.connect(path)
    conn.row_factory = sqlite3.Row
    _harden_connection(conn)
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
