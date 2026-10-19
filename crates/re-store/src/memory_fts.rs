//! Opt-in BM25 memory recall over SQLite FTS5 (`REASONING_ENGINE_MEMORY=fts`,
//! plan Phase 5).
//!
//! The index is a per-connection `temp` table rebuilt from `episodic_memory`
//! on each recall, so the database file never gains a virtual table or a
//! trigger. That keeps it openable by the Python implementation, whose
//! system SQLite may lack FTS5, and means memories Python writes are always
//! indexed. Episodic memory holds hundreds of rows, not millions, so the
//! rebuild costs well under a millisecond per hundred memories.
//!
//! Unlike the legacy word overlap, the index also covers key learnings, and
//! ranks by BM25 rather than by a count of shared words.

use std::collections::HashSet;

use re_core::rank::tokenize;
use rusqlite::params;

use crate::conn::Store;
use crate::error::StoreError;
use crate::memory::RecalledMemory;

/// An FTS5 query matching any token of `query`, each quoted so user text
/// can never become FTS5 syntax (`NEAR`, `*`, column filters, ...).
fn match_expression(query: &str) -> Option<String> {
    let mut seen = HashSet::new();
    let terms: Vec<String> = tokenize(query)
        .into_iter()
        .filter(|t| seen.insert(t.clone()))
        .map(|t| format!("\"{}\"", t.replace('"', "\"\"")))
        .collect();
    (!terms.is_empty()).then(|| terms.join(" OR "))
}

impl Store {
    /// Memories matching any word of `query`, best BM25 first.
    ///
    /// `relevance` is the number of distinct query words the memory
    /// contains (in its query, learnings or tags).
    ///
    /// # Errors
    /// A corrupt JSON column, or `SQLite` failures.
    pub fn recall_memory_fts(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<RecalledMemory>, StoreError> {
        let Some(expression) = match_expression(query) else {
            return Ok(Vec::new());
        };
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let rows: Vec<(String, String, String, String, String, String)> = self.with_tx(|tx| {
            tx.execute_batch(
                "CREATE VIRTUAL TABLE IF NOT EXISTS temp.episodic_memory_fts USING fts5(
                    memory_id UNINDEXED, query, key_learnings, domain_tags,
                    tokenize = 'unicode61 remove_diacritics 2'
                 );
                 DELETE FROM temp.episodic_memory_fts;
                 INSERT INTO temp.episodic_memory_fts (memory_id, query, key_learnings, domain_tags)
                    SELECT id, query, key_learnings, domain_tags FROM episodic_memory;",
            )?;
            let mut stmt = tx.prepare(
                "SELECT m.id, m.session_id, m.query, m.key_learnings, m.domain_tags, m.created_at
                 FROM temp.episodic_memory_fts(?1) AS f
                 JOIN episodic_memory AS m ON m.id = f.memory_id
                 ORDER BY f.rank, m.rowid
                 LIMIT ?2",
            )?;
            let rows = stmt
                .query_map(params![expression, limit], |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })?;
        let query_terms: HashSet<String> = tokenize(query).into_iter().collect();
        rows.into_iter()
            .map(
                |(id, session_id, stored_query, learnings, tags, created_at)| {
                    let key_learnings: Vec<String> = parse(&learnings, "key_learnings")?;
                    let domain_tags: Vec<String> = parse(&tags, "domain_tags")?;
                    let text = format!(
                        "{stored_query} {} {}",
                        key_learnings.join(" "),
                        domain_tags.join(" ")
                    );
                    let words: HashSet<String> = tokenize(&text).into_iter().collect();
                    Ok(RecalledMemory {
                        id,
                        session_id,
                        query: stored_query,
                        key_learnings,
                        domain_tags,
                        created_at,
                        relevance: query_terms.intersection(&words).count(),
                    })
                },
            )
            .collect()
    }
}

fn parse(text: &str, column: &'static str) -> Result<Vec<String>, StoreError> {
    serde_json::from_str(text).map_err(|source| StoreError::Json { column, source })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_text_cannot_inject_fts_syntax() {
        assert_eq!(
            match_expression("PRM* NEAR(a b) \"x"),
            Some("\"prm\" OR \"near\" OR \"a\" OR \"b\" OR \"x\"".into())
        );
        assert_eq!(match_expression("  ?! "), None);
        assert_eq!(match_expression("reward reward"), Some("\"reward\"".into()));
    }
}
