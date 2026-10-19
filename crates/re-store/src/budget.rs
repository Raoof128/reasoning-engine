//! Mid-run budget re-estimation records (plan Phase 10).
//!
//! A session's budget is re-estimated at most once: [`Store::reestimate_budget`]
//! updates the session's difficulty, strategy and steps and records the
//! change in `budget_reestimates`, atomically, and does nothing if the
//! session already has a record. The table is created on first use, so a
//! database the Python implementation shares never gains it unless the
//! feature runs.

use rusqlite::{OptionalExtension, params};

use crate::conn::Store;
use crate::error::StoreError;
use crate::sessions::session_not_found;

const CREATE: &str = "CREATE TABLE IF NOT EXISTS budget_reestimates (
    session_id TEXT PRIMARY KEY REFERENCES sessions(id),
    prior_difficulty REAL NOT NULL,
    observed_difficulty REAL NOT NULL,
    from_strategy TEXT NOT NULL,
    to_strategy TEXT NOT NULL,
    step_delta INTEGER NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
)";

/// A budget change to apply once.
#[derive(Debug, Clone, PartialEq)]
pub struct BudgetChange<'a> {
    /// Difficulty the budget was planned from.
    pub prior_difficulty: f64,
    /// Difficulty after the first round.
    pub observed_difficulty: f64,
    /// Strategy before.
    pub from_strategy: &'a str,
    /// Strategy after.
    pub to_strategy: &'a str,
    /// Steps added (negative: removed).
    pub step_delta: i64,
}

impl Store {
    /// Whether `session_id`'s budget was already re-estimated.
    ///
    /// # Errors
    /// `SQLite` failures.
    pub fn budget_reestimated(&self, session_id: &str) -> Result<bool, StoreError> {
        self.with_tx(|tx| {
            tx.execute_batch(CREATE)?;
            Ok(tx
                .query_row(
                    "SELECT 1 FROM budget_reestimates WHERE session_id = ?",
                    [session_id],
                    |_| Ok(()),
                )
                .optional()?
                .is_some())
        })
    }

    /// Apply `change` to the session (difficulty, strategy, maximum and
    /// remaining steps, never below zero) and record it. Returns the new
    /// remaining steps, or `None` when the session was already
    /// re-estimated.
    ///
    /// # Errors
    /// An unknown session, or `SQLite` failures.
    pub fn reestimate_budget(
        &self,
        session_id: &str,
        change: &BudgetChange<'_>,
    ) -> Result<Option<i64>, StoreError> {
        self.with_tx(|tx| {
            tx.execute_batch(CREATE)?;
            let remaining: Option<(i64, i64)> = tx
                .query_row(
                    "SELECT budget_remaining_steps, budget_max_steps FROM sessions WHERE id = ?",
                    [session_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;
            let Some((remaining, max_steps)) = remaining else {
                return Err(session_not_found(session_id));
            };
            let inserted = tx.execute(
                "INSERT OR IGNORE INTO budget_reestimates
                    (session_id, prior_difficulty, observed_difficulty, from_strategy,
                     to_strategy, step_delta)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    session_id,
                    change.prior_difficulty,
                    change.observed_difficulty,
                    change.from_strategy,
                    change.to_strategy,
                    change.step_delta
                ],
            )?;
            if inserted == 0 {
                return Ok(None);
            }
            let remaining = (remaining + change.step_delta).max(0);
            tx.execute(
                "UPDATE sessions SET difficulty = ?2, strategy = ?3,
                    budget_max_steps = ?4, budget_remaining_steps = ?5 WHERE id = ?1",
                params![
                    session_id,
                    change.observed_difficulty,
                    change.to_strategy,
                    (max_steps + change.step_delta).max(0),
                    remaining
                ],
            )?;
            Ok(Some(remaining))
        })
    }
}
