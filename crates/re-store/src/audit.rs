//! Read-only listings for the audit UI (`re-ui`, plan Phase 6). Nothing
//! here writes; Python has no equivalent.

use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::conn::Store;
use crate::error::StoreError;
use crate::research::RunRow;
use crate::sessions::{SessionRow, session_row};

/// A `reflections` row, columns in table order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReflectionRow {
    /// Reflection ID.
    pub id: String,
    /// Reflected branch.
    pub branch_id: String,
    /// Owning session.
    pub session_id: String,
    /// Critique that prompted the revision.
    pub original_critique: String,
    /// What changed.
    pub revision_summary: String,
    /// Score before the revision.
    pub score_before: f64,
    /// Score after the revision.
    pub score_after: f64,
    /// `SQLite` `datetime('now')`.
    pub created_at: String,
}

fn clamp(limit: usize) -> i64 {
    i64::try_from(limit).unwrap_or(i64::MAX)
}

impl Store {
    /// The newest sessions first, at most `limit`.
    ///
    /// # Errors
    /// `SQLite` failures.
    pub fn list_sessions(&self, limit: usize) -> Result<Vec<SessionRow>, StoreError> {
        self.with_tx(|tx| {
            let mut stmt =
                tx.prepare("SELECT * FROM sessions ORDER BY created_at DESC, rowid DESC LIMIT ?")?;
            let rows = stmt
                .query_map([clamp(limit)], session_row)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    /// The newest research runs first, at most `limit`.
    ///
    /// # Errors
    /// `SQLite` failures.
    pub fn list_runs(&self, limit: usize) -> Result<Vec<RunRow>, StoreError> {
        self.with_tx(|tx| {
            let mut stmt = tx.prepare(
                "SELECT run_id, query, profile, mode, status, created_at FROM research_runs
                 ORDER BY created_at DESC, rowid DESC LIMIT ?",
            )?;
            let rows = stmt
                .query_map([clamp(limit)], |row| {
                    Ok(RunRow {
                        run_id: row.get(0)?,
                        query: row.get(1)?,
                        profile: row.get(2)?,
                        mode: row.get(3)?,
                        status: row.get(4)?,
                        created_at: row.get(5)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    /// A session's reflections, oldest first.
    ///
    /// # Errors
    /// `SQLite` failures.
    pub fn list_reflections(&self, session_id: &str) -> Result<Vec<ReflectionRow>, StoreError> {
        self.with_tx(|tx| {
            let mut stmt = tx.prepare(
                "SELECT id, branch_id, session_id, original_critique, revision_summary,
                        score_before, score_after, created_at
                 FROM reflections WHERE session_id = ? ORDER BY created_at, rowid",
            )?;
            let rows = stmt
                .query_map(params![session_id], |row| {
                    Ok(ReflectionRow {
                        id: row.get(0)?,
                        branch_id: row.get(1)?,
                        session_id: row.get(2)?,
                        original_critique: row.get(3)?,
                        revision_summary: row.get(4)?,
                        score_before: row.get(5)?,
                        score_after: row.get(6)?,
                        created_at: row.get(7)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }
}
