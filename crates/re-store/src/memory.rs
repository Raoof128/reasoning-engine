//! Episodic memory and reflexion records (port of memory.py).

use std::collections::HashSet;

use re_core::sanitize::sanitize_content;
use re_model::ids::new_uuid;
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::conn::Store;
use crate::error::StoreError;
use crate::sessions::{dumps_strings, require_branch_for_session, session_not_found};

/// What `save_memory` returns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemorySaved {
    /// Memory ID (uuid4).
    pub id: String,
}

/// A recalled memory: the row with JSON columns parsed, plus `_relevance`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecalledMemory {
    /// Memory ID.
    pub id: String,
    /// Session that saved it.
    pub session_id: String,
    /// Sanitised query.
    pub query: String,
    /// Sanitised learnings.
    pub key_learnings: Vec<String>,
    /// Sanitised tags.
    pub domain_tags: Vec<String>,
    /// `SQLite` `datetime('now')`.
    pub created_at: String,
    /// Count of shared words between the query and the stored query plus tags.
    #[serde(rename = "_relevance")]
    pub relevance: usize,
}

/// What `record_reflection` returns.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReflectionRecorded {
    /// Reflection ID (uuid4).
    pub id: String,
    /// Score before the revision.
    pub score_before: f64,
    /// Score after the revision.
    pub score_after: f64,
}

fn parse_strings(text: &str, column: &'static str) -> Result<Vec<String>, StoreError> {
    serde_json::from_str(text).map_err(|source| StoreError::Json { column, source })
}

impl Store {
    /// Save learnings from a session for future recall.
    ///
    /// # Errors
    /// Unknown session, or `SQLite` failures.
    pub fn save_memory(
        &self,
        session_id: &str,
        query: &str,
        key_learnings: &[String],
        domain_tags: &[String],
    ) -> Result<MemorySaved, StoreError> {
        let id = new_uuid();
        let sanitize_all = |items: &[String]| {
            items
                .iter()
                .map(|s| sanitize_content(s))
                .collect::<Vec<_>>()
        };
        let learnings = dumps_strings(&sanitize_all(key_learnings));
        let tags = dumps_strings(&sanitize_all(domain_tags));
        let query = sanitize_content(query);
        self.with_tx(|tx| {
            tx.query_row("SELECT id FROM sessions WHERE id = ?", [session_id], |_| {
                Ok(())
            })
            .optional()?
            .ok_or_else(|| session_not_found(session_id))?;
            tx.execute(
                "INSERT INTO episodic_memory (id, session_id, query, key_learnings, domain_tags)
                   VALUES (?, ?, ?, ?, ?)",
                params![id, session_id, query, learnings, tags],
            )?;
            Ok(())
        })?;
        Ok(MemorySaved { id })
    }

    /// Memories sharing at least one word with `query`, most relevant first.
    ///
    /// Words are whitespace-split and lowercased; tags also contribute each
    /// `-`-separated part. Ties keep storage order, like Python's stable sort.
    ///
    /// # Errors
    /// A corrupt JSON column, or `SQLite` failures.
    pub fn recall_memory(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<RecalledMemory>, StoreError> {
        let lowered = query.to_lowercase();
        let query_words: HashSet<&str> = lowered.split_whitespace().collect();
        let rows: Vec<(String, String, String, String, String, String)> = self.with_tx(|tx| {
            let mut stmt = tx.prepare(
                "SELECT id, session_id, query, key_learnings, domain_tags, created_at FROM episodic_memory",
            )?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })?;

        let mut scored = Vec::new();
        for (id, session_id, stored_query, learnings, tags, created_at) in rows {
            let domain_tags = parse_strings(&tags, "domain_tags")?;
            let stored_lower = stored_query.to_lowercase();
            let mut stored_words: HashSet<String> = stored_lower
                .split_whitespace()
                .map(str::to_string)
                .collect();
            for tag in &domain_tags {
                let tag = tag.to_lowercase();
                stored_words.extend(tag.split('-').map(str::to_string));
                stored_words.insert(tag);
            }
            let overlap = query_words
                .iter()
                .filter(|w| stored_words.contains(**w))
                .count();
            if overlap > 0 {
                scored.push(RecalledMemory {
                    id,
                    session_id,
                    query: stored_query,
                    key_learnings: parse_strings(&learnings, "key_learnings")?,
                    domain_tags,
                    created_at,
                    relevance: overlap,
                });
            }
        }
        // Stable, so equal relevance keeps storage order like Python's sort.
        scored.sort_by_key(|m| std::cmp::Reverse(m.relevance));
        scored.truncate(limit);
        Ok(scored)
    }

    /// Store a reflexion cycle and reactivate the branch.
    ///
    /// # Errors
    /// A branch outside `session_id`, or `SQLite` failures.
    pub fn record_reflection(
        &self,
        branch_id: &str,
        session_id: &str,
        original_critique: &str,
        revision_summary: &str,
        score_before: f64,
        score_after: f64,
    ) -> Result<ReflectionRecorded, StoreError> {
        let id = new_uuid();
        let critique = sanitize_content(original_critique);
        let summary = sanitize_content(revision_summary);
        self.with_tx(|tx| {
            require_branch_for_session(tx, branch_id, session_id)?;
            tx.execute(
                "INSERT INTO reflections (id, branch_id, session_id, original_critique, revision_summary, score_before, score_after)
                   VALUES (?, ?, ?, ?, ?, ?, ?)",
                params![id, branch_id, session_id, critique, summary, score_before, score_after],
            )?;
            tx.execute(
                "UPDATE branches SET status='active' WHERE id=? AND session_id=?",
                params![branch_id, session_id],
            )?;
            Ok(())
        })?;
        Ok(ReflectionRecorded {
            id,
            score_before,
            score_after,
        })
    }
}
