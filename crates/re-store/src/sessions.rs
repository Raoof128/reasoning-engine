//! Session and branch lifecycle (port of sessions.py).
//!
//! SQL text, ordering and error messages match Python. Branch traces and
//! critiques are sanitised on the way in, exactly where Python does it;
//! the session query is not, because Python validates it at the MCP layer.

use re_core::difficulty::estimate_difficulty;
use re_core::dora::allocate_budget;
use re_core::pyjson::{DumpOptions, dumps};
use re_core::sanitize::sanitize_content;
use re_model::ids::new_uuid;
use rusqlite::{OptionalExtension, Row, Transaction, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::conn::Store;
use crate::error::StoreError;

/// `q_score` a branch must exceed to end the search early.
pub const TERMINATION_Q_THRESHOLD: f64 = 0.85;
/// `confidence` a branch must exceed to end the search early.
pub const TERMINATION_CONFIDENCE_THRESHOLD: f64 = 0.80;
/// Statuses `update_branch_status` accepts.
pub const VALID_BRANCH_STATUSES: &[&str] = &["active", "reflecting", "pruned", "completed"];

/// A `sessions` row, columns in table order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionRow {
    /// Session ID (uuid4).
    pub id: String,
    /// Research question.
    pub query: String,
    /// Estimated difficulty.
    pub difficulty: f64,
    /// Search strategy.
    pub strategy: String,
    /// Branch budget.
    pub budget_total_branches: i64,
    /// Depth budget.
    pub budget_max_depth: i64,
    /// Step budget.
    pub budget_max_steps: i64,
    /// Token budget per branch.
    pub budget_tokens_per_branch: i64,
    /// Planner steps left.
    pub budget_remaining_steps: i64,
    /// `active` or `completed`.
    pub status: String,
    /// `SQLite` `datetime('now')`.
    pub created_at: String,
}

/// A `branches` row, columns in table order. `trace` is the stored JSON text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BranchRow {
    /// Branch ID (uuid4).
    pub id: String,
    /// Owning session.
    pub session_id: String,
    /// Parent branch, if any.
    pub parent_id: Option<String>,
    /// Tree depth.
    pub depth: i64,
    /// JSON array of sanitised steps, as stored.
    pub trace: String,
    /// Promise score.
    pub q_score: f64,
    /// Progress score.
    pub advantage: f64,
    /// Sanitised critique.
    pub critique: String,
    /// Critic confidence.
    pub confidence: f64,
    /// Times scored.
    pub visits: i64,
    /// Lifecycle status.
    pub status: String,
    /// `SQLite` `datetime('now')`.
    pub created_at: String,
}

/// The budget block of [`SessionCreated`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetView {
    /// Branches to generate.
    pub total_branches: i64,
    /// Maximum depth.
    pub max_depth: i64,
    /// Maximum planner steps.
    pub max_steps: i64,
    /// Token budget per branch.
    pub tokens_per_branch: i64,
    /// Steps left (equal to `max_steps` at creation).
    pub remaining_steps: i64,
}

/// What `create_session` returns.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionCreated {
    /// Session ID.
    pub id: String,
    /// Research question.
    pub query: String,
    /// Estimated difficulty.
    pub difficulty: f64,
    /// Search strategy.
    pub strategy: String,
    /// Allocated budget.
    pub budget: BudgetView,
    /// Always `active`.
    pub status: String,
}

/// What `register_branch` returns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchCreated {
    /// Branch ID.
    pub id: String,
    /// Owning session.
    pub session_id: String,
    /// Parent branch.
    pub parent_id: Option<String>,
    /// Tree depth.
    pub depth: i64,
}

/// A source attached to a branch. Validation happens at the MCP layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct SourceInput {
    /// `http(s)` URL; stored as given.
    #[serde(default)]
    pub url: String,
    /// Title; sanitised before storage.
    #[serde(default)]
    pub title: String,
    /// Excerpt; sanitised before storage.
    #[serde(default)]
    pub excerpt: String,
    /// Relevance in `[0, 1]`.
    #[serde(default)]
    pub relevance_score: f64,
}

/// What `check_termination` returns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Termination {
    /// Whether to stop searching.
    pub should_terminate: bool,
    /// Why.
    pub reason: String,
}

pub(crate) fn session_not_found(id: &str) -> StoreError {
    StoreError::invalid(format!("Session {id} not found"))
}

fn branch_not_found(id: &str) -> StoreError {
    StoreError::invalid(format!("Branch {id} not found"))
}

pub(crate) fn branch_not_in_session(branch_id: &str, session_id: &str) -> StoreError {
    StoreError::invalid(format!(
        "Branch {branch_id} not found in session {session_id}"
    ))
}

fn session_row(row: &Row<'_>) -> rusqlite::Result<SessionRow> {
    Ok(SessionRow {
        id: row.get("id")?,
        query: row.get("query")?,
        difficulty: row.get("difficulty")?,
        strategy: row.get("strategy")?,
        budget_total_branches: row.get("budget_total_branches")?,
        budget_max_depth: row.get("budget_max_depth")?,
        budget_max_steps: row.get("budget_max_steps")?,
        budget_tokens_per_branch: row.get("budget_tokens_per_branch")?,
        budget_remaining_steps: row.get("budget_remaining_steps")?,
        status: row.get("status")?,
        created_at: row.get("created_at")?,
    })
}

fn branch_row(row: &Row<'_>) -> rusqlite::Result<BranchRow> {
    Ok(BranchRow {
        id: row.get("id")?,
        session_id: row.get("session_id")?,
        parent_id: row.get("parent_id")?,
        depth: row.get("depth")?,
        trace: row.get("trace")?,
        q_score: row.get("q_score")?,
        advantage: row.get("advantage")?,
        critique: row.get("critique")?,
        confidence: row.get("confidence")?,
        visits: row.get("visits")?,
        status: row.get("status")?,
        created_at: row.get("created_at")?,
    })
}

/// `json.dumps(list_of_strings)` with Python's default separators.
pub(crate) fn dumps_strings(items: &[String]) -> String {
    let value = Value::Array(items.iter().cloned().map(Value::String).collect());
    dumps(&value, &DumpOptions::sorted())
}

pub(crate) fn require_branch_for_session(
    tx: &Transaction<'_>,
    branch_id: &str,
    session_id: &str,
) -> Result<BranchRow, StoreError> {
    tx.query_row(
        "SELECT * FROM branches WHERE id = ? AND session_id = ?",
        params![branch_id, session_id],
        branch_row,
    )
    .optional()?
    .ok_or_else(|| branch_not_in_session(branch_id, session_id))
}

fn session_exists(tx: &Transaction<'_>, session_id: &str) -> Result<bool, StoreError> {
    Ok(tx
        .query_row("SELECT id FROM sessions WHERE id = ?", [session_id], |_| {
            Ok(())
        })
        .optional()?
        .is_some())
}

impl Store {
    /// Create a session with an estimated difficulty and budget.
    ///
    /// # Errors
    /// `SQLite` failures.
    pub fn create_session(&self, query: &str) -> Result<SessionCreated, StoreError> {
        let id = new_uuid();
        let difficulty = estimate_difficulty(query);
        let budget = allocate_budget(difficulty);
        self.with_tx(|tx| {
            tx.execute(
                "INSERT INTO sessions
                   (id, query, difficulty, strategy,
                    budget_total_branches, budget_max_depth, budget_max_steps,
                    budget_tokens_per_branch, budget_remaining_steps, status)
                   VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'active')",
                params![
                    id,
                    query,
                    difficulty,
                    budget.strategy.as_str(),
                    budget.total_branches,
                    budget.max_depth,
                    budget.max_steps,
                    budget.tokens_per_branch,
                    budget.max_steps,
                ],
            )?;
            Ok(())
        })?;
        Ok(SessionCreated {
            id,
            query: query.to_string(),
            difficulty,
            strategy: budget.strategy.as_str().to_string(),
            budget: BudgetView {
                total_branches: budget.total_branches.into(),
                max_depth: budget.max_depth.into(),
                max_steps: budget.max_steps.into(),
                tokens_per_branch: budget.tokens_per_branch.into(),
                remaining_steps: budget.max_steps.into(),
            },
            status: "active".to_string(),
        })
    }

    /// Fetch a session.
    ///
    /// # Errors
    /// `Session <id> not found`, or `SQLite` failures.
    pub fn get_session(&self, session_id: &str) -> Result<SessionRow, StoreError> {
        self.with_tx(|tx| {
            tx.query_row(
                "SELECT * FROM sessions WHERE id = ?",
                [session_id],
                session_row,
            )
            .optional()?
            .ok_or_else(|| session_not_found(session_id))
        })
    }

    /// Fetch a branch, checking it belongs to `session_id`.
    ///
    /// # Errors
    /// `Branch <id> not found in session <sid>`, or `SQLite` failures.
    pub fn get_branch_for_session(
        &self,
        branch_id: &str,
        session_id: &str,
    ) -> Result<BranchRow, StoreError> {
        self.with_tx(|tx| require_branch_for_session(tx, branch_id, session_id))
    }

    /// Register a branch with its trace and sources.
    ///
    /// # Errors
    /// Unknown session, depth over the session's maximum, a parent from
    /// another session, or `SQLite` failures.
    pub fn register_branch(
        &self,
        session_id: &str,
        trace: &[String],
        sources: &[SourceInput],
        parent_id: Option<&str>,
        depth: i64,
    ) -> Result<BranchCreated, StoreError> {
        let id = new_uuid();
        let trace_json = dumps_strings(
            &trace
                .iter()
                .map(|s| sanitize_content(s))
                .collect::<Vec<_>>(),
        );
        self.with_tx(|tx| {
            let max_depth: i64 = tx
                .query_row(
                    "SELECT budget_max_depth FROM sessions WHERE id = ?",
                    [session_id],
                    |row| row.get(0),
                )
                .optional()?
                .ok_or_else(|| session_not_found(session_id))?;
            if depth > max_depth {
                return Err(StoreError::invalid(format!(
                    "depth exceeds session max depth {max_depth}"
                )));
            }
            if let Some(parent) = parent_id {
                require_branch_for_session(tx, parent, session_id)?;
            }
            tx.execute(
                "INSERT INTO branches (id, session_id, parent_id, depth, trace, status)
                   VALUES (?, ?, ?, ?, ?, 'active')",
                params![id, session_id, parent_id, depth, trace_json],
            )?;
            for source in sources {
                tx.execute(
                    "INSERT INTO sources (id, branch_id, url, title, excerpt, relevance_score)
                       VALUES (?, ?, ?, ?, ?, ?)",
                    params![
                        new_uuid(),
                        id,
                        source.url,
                        sanitize_content(&source.title),
                        sanitize_content(&source.excerpt),
                        source.relevance_score,
                    ],
                )?;
            }
            Ok(())
        })?;
        Ok(BranchCreated {
            id,
            session_id: session_id.to_string(),
            parent_id: parent_id.map(str::to_string),
            depth,
        })
    }

    /// Fetch a branch by ID alone.
    ///
    /// # Errors
    /// `Branch <id> not found`, or `SQLite` failures.
    pub fn get_branch(&self, branch_id: &str) -> Result<BranchRow, StoreError> {
        self.with_tx(|tx| {
            tx.query_row(
                "SELECT * FROM branches WHERE id = ?",
                [branch_id],
                branch_row,
            )
            .optional()?
            .ok_or_else(|| branch_not_found(branch_id))
        })
    }

    /// Record the critic's dual-signal score and bump the visit count.
    ///
    /// # Errors
    /// A branch outside `session_id` (when given), an unknown branch, or
    /// `SQLite` failures.
    pub fn score_branch(
        &self,
        branch_id: &str,
        q_score: f64,
        advantage: f64,
        critique: &str,
        confidence: f64,
        session_id: Option<&str>,
    ) -> Result<(), StoreError> {
        let critique = sanitize_content(critique);
        self.with_tx(|tx| {
            if let Some(session_id) = session_id {
                require_branch_for_session(tx, branch_id, session_id)?;
            }
            let changed = tx.execute(
                "UPDATE branches SET q_score=?, advantage=?, critique=?, confidence=?, visits=visits+1 WHERE id=?",
                params![q_score, advantage, critique, confidence, branch_id],
            )?;
            if changed == 0 {
                return Err(branch_not_found(branch_id));
            }
            Ok(())
        })
    }

    /// Set a branch's status.
    ///
    /// # Errors
    /// `Invalid branch status: <s>`, an unknown branch, or `SQLite` failures.
    pub fn update_branch_status(&self, branch_id: &str, status: &str) -> Result<(), StoreError> {
        if !VALID_BRANCH_STATUSES.contains(&status) {
            return Err(StoreError::invalid(format!(
                "Invalid branch status: {status}"
            )));
        }
        self.with_tx(|tx| {
            let changed = tx.execute(
                "UPDATE branches SET status=? WHERE id=?",
                params![status, branch_id],
            )?;
            if changed == 0 {
                return Err(branch_not_found(branch_id));
            }
            Ok(())
        })
    }

    /// Persist a DORA decision atomically: flag branches, prune branches and
    /// store the remaining budget, completing the session when it hits zero.
    ///
    /// # Errors
    /// Unknown session, a branch from another session, or `SQLite` failures.
    /// Nothing is written unless every check passes.
    pub fn apply_planning_result(
        &self,
        session_id: &str,
        branches_to_reflect: &[String],
        branches_to_prune: &[String],
        budget_remaining: i64,
    ) -> Result<(), StoreError> {
        self.with_tx(|tx| {
            if !session_exists(tx, session_id)? {
                return Err(session_not_found(session_id));
            }
            for branch_id in branches_to_reflect {
                require_branch_for_session(tx, branch_id, session_id)?;
                tx.execute(
                    "UPDATE branches SET status='reflecting' WHERE id=?",
                    [branch_id],
                )?;
            }
            for branch_id in branches_to_prune {
                require_branch_for_session(tx, branch_id, session_id)?;
                tx.execute(
                    "UPDATE branches SET status='pruned' WHERE id=?",
                    [branch_id],
                )?;
            }
            let status = if budget_remaining <= 0 {
                "completed"
            } else {
                "active"
            };
            tx.execute(
                "UPDATE sessions SET budget_remaining_steps=?, status=? WHERE id=?",
                params![budget_remaining.max(0), status, session_id],
            )?;
            Ok(())
        })
    }

    fn query_branches(
        &self,
        sql: &str,
        params: impl rusqlite::Params,
    ) -> Result<Vec<BranchRow>, StoreError> {
        self.with_tx(|tx| {
            let mut stmt = tx.prepare(sql)?;
            let rows = stmt
                .query_map(params, branch_row)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    /// Active branches of a session.
    ///
    /// # Errors
    /// `SQLite` failures.
    pub fn get_active_branches(&self, session_id: &str) -> Result<Vec<BranchRow>, StoreError> {
        self.query_branches(
            "SELECT * FROM branches WHERE session_id=? AND status='active'",
            [session_id],
        )
    }

    /// Every branch of a session, oldest first.
    ///
    /// # Errors
    /// `SQLite` failures.
    pub fn get_session_branches(&self, session_id: &str) -> Result<Vec<BranchRow>, StoreError> {
        self.query_branches(
            "SELECT * FROM branches WHERE session_id=? ORDER BY created_at, id",
            [session_id],
        )
    }

    /// Stop when the budget is spent or an active branch is both promising
    /// and confident.
    ///
    /// # Errors
    /// `SQLite` failures.
    pub fn check_termination(
        &self,
        session_id: &str,
        budget_remaining: i64,
    ) -> Result<Termination, StoreError> {
        if budget_remaining <= 0 {
            return Ok(Termination {
                should_terminate: true,
                reason: "Budget exhausted".to_string(),
            });
        }
        let top: Option<(f64, f64)> = self.with_tx(|tx| {
            Ok(tx
                .query_row(
                    "SELECT q_score, confidence FROM branches WHERE session_id=? AND status='active' ORDER BY q_score DESC LIMIT 1",
                    [session_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?)
        })?;
        if let Some((q, conf)) = top
            && q > TERMINATION_Q_THRESHOLD
            && conf > TERMINATION_CONFIDENCE_THRESHOLD
        {
            return Ok(Termination {
                should_terminate: true,
                reason: format!("High confidence result (q={q:.2}, conf={conf:.2})"),
            });
        }
        Ok(Termination {
            should_terminate: false,
            reason: "Continue".to_string(),
        })
    }

    /// The `top_k` best active or completed branches.
    ///
    /// # Errors
    /// `SQLite` failures.
    pub fn get_consensus_candidates(
        &self,
        session_id: &str,
        top_k: i64,
    ) -> Result<Vec<BranchRow>, StoreError> {
        self.query_branches(
            "SELECT * FROM branches WHERE session_id=? AND status IN ('active','completed') ORDER BY q_score DESC LIMIT ?",
            params![session_id, top_k],
        )
    }
}
