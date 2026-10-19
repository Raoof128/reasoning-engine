//! Training data for the learned prior (`learn` feature, plan Phase 11).
//!
//! A session teaches the prior once at least [`MIN_SCORED`] of its branches
//! were scored: each scored branch is an example, labelled by whether it
//! ranks in the top [`re_core::prior::CONSENSUS_TOP_K`] by score, as
//! `consensus_candidates` would return it.

use re_core::prior::{BranchFeatures, CONSENSUS_TOP_K, Example};
use rusqlite::params;

use crate::conn::Store;
use crate::error::StoreError;

/// Scored branches a session needs to teach anything.
pub const MIN_SCORED: usize = 2;

struct Row {
    session_id: String,
    features: BranchFeatures,
}

fn steps(trace: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(trace).unwrap_or_default()
}

impl Store {
    /// Every scored branch of every session with at least [`MIN_SCORED`]
    /// of them, labelled, in session then score order.
    ///
    /// # Errors
    /// `SQLite` failures.
    pub fn training_examples(&self) -> Result<Vec<Example>, StoreError> {
        let rows: Vec<Row> = self.with_tx(|tx| {
            let mut statement = tx.prepare(
                "SELECT b.session_id, b.trace, b.depth, b.parent_id, s.difficulty,
                        (SELECT COUNT(*) FROM sources WHERE branch_id = b.id)
                 FROM branches b JOIN sessions s ON s.id = b.session_id
                 WHERE b.visits > 0
                 ORDER BY b.session_id, b.q_score DESC, b.created_at, b.id",
            )?;
            let rows = statement
                .query_map([], |row| {
                    let trace: String = row.get(1)?;
                    let parent: Option<String> = row.get(3)?;
                    let sources: i64 = row.get(5)?;
                    Ok(Row {
                        session_id: row.get(0)?,
                        features: BranchFeatures {
                            trace: steps(&trace),
                            depth: row.get(2)?,
                            sources: usize::try_from(sources).unwrap_or(0),
                            has_parent: parent.is_some_and(|p| !p.is_empty()),
                            difficulty: row.get(4)?,
                        },
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })?;
        let mut examples = Vec::with_capacity(rows.len());
        let mut start = 0;
        while start < rows.len() {
            let session = &rows[start].session_id;
            let end = rows[start..]
                .iter()
                .position(|r| &r.session_id != session)
                .map_or(rows.len(), |offset| start + offset);
            if end - start >= MIN_SCORED {
                for (rank, row) in rows[start..end].iter().enumerate() {
                    examples.push(Example {
                        session_id: row.session_id.clone(),
                        features: row.features.clone(),
                        in_consensus: rank < CONSENSUS_TOP_K,
                    });
                }
            }
            start = end;
        }
        Ok(examples)
    }

    /// What the prior sees of `branch_id`.
    ///
    /// # Errors
    /// An unknown branch, or `SQLite` failures.
    pub fn branch_features(&self, branch_id: &str) -> Result<BranchFeatures, StoreError> {
        self.with_tx(|tx| {
            Ok(tx.query_row(
                "SELECT b.trace, b.depth, b.parent_id, s.difficulty,
                        (SELECT COUNT(*) FROM sources WHERE branch_id = b.id)
                 FROM branches b JOIN sessions s ON s.id = b.session_id WHERE b.id = ?",
                params![branch_id],
                |row| {
                    let trace: String = row.get(0)?;
                    let parent: Option<String> = row.get(2)?;
                    let sources: i64 = row.get(4)?;
                    Ok(BranchFeatures {
                        trace: steps(&trace),
                        depth: row.get(1)?,
                        sources: usize::try_from(sources).unwrap_or(0),
                        has_parent: parent.is_some_and(|p| !p.is_empty()),
                        difficulty: row.get(3)?,
                    })
                },
            )?)
        })
    }

    /// Seed an unscored branch's score with the prior's estimate. Scored
    /// branches are left alone.
    ///
    /// # Errors
    /// `SQLite` failures.
    pub fn seed_branch_score(&self, branch_id: &str, q_score: f64) -> Result<(), StoreError> {
        self.with_tx(|tx| {
            tx.execute(
                "UPDATE branches SET q_score = ?2 WHERE id = ?1 AND visits = 0",
                params![branch_id, q_score],
            )?;
            Ok(())
        })
    }
}
