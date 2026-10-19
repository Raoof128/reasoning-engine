//! The 13-table schema, copied verbatim from `db.py`.
//!
//! Every statement is `IF NOT EXISTS`, so opening a database Python created
//! changes nothing. Python never writes `quality_gate_results` or
//! `attestation_manifests`; they are created for compatibility only.

/// Schema version Python stamps into `PRAGMA user_version`. Version 3 keys
/// `claims` and `claim_verifications` by run (see [`CLAIMS_V2_CHECK`]).
pub const PYTHON_USER_VERSION: i64 = 3;

/// Every table the engine uses.
pub const TABLES: &[&str] = &[
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
];

/// DDL for all tables and indexes.
pub const SCHEMA_SQL: &str = r"
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
";

/// Primary-key columns of `claims` plus `claim_verifications`: 4 once both
/// are keyed by `(run_id, ...)`, 2 for the version-2 tables keyed by the bare
/// claim and verification IDs. Claim IDs restart at `claim_0001` in every
/// run, so under version 2 a second run overwrote the first run's claims.
///
/// The check reads the schema, not `user_version`, because older builds of
/// either implementation reset the version to 2 when they open a file.
pub const CLAIMS_V2_CHECK: &str = "SELECT
    (SELECT COUNT(*) FROM pragma_table_info('claims') WHERE pk > 0)
  + (SELECT COUNT(*) FROM pragma_table_info('claim_verifications') WHERE pk > 0)";

/// First half of the version-2 migration: move the old tables aside so
/// [`SCHEMA_SQL`] can create the run-keyed ones under the same names.
pub const CLAIMS_V2_SET_ASIDE: &str = "
DROP INDEX IF EXISTS idx_claims_run;
DROP INDEX IF EXISTS idx_verifications_run;
ALTER TABLE claim_verifications RENAME TO claim_verifications_v2;
ALTER TABLE claims RENAME TO claims_v2;
";

/// Second half: copy every row into the run-keyed tables, then drop the old
/// ones. Verifications whose claim a later run already overwrote are kept;
/// they stay readable by run even though their claim row is gone.
pub const CLAIMS_V2_COPY: &str = "
INSERT INTO claims (claim_id, run_id, payload, status, created_at)
    SELECT claim_id, run_id, payload, status, created_at FROM claims_v2;
INSERT INTO claim_verifications
    (verification_id, claim_id, run_id, payload, support_status, created_at)
    SELECT verification_id, claim_id, run_id, payload, support_status, created_at
    FROM claim_verifications_v2;
DROP TABLE claim_verifications_v2;
DROP TABLE claims_v2;
";
