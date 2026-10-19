//! Cross-session evidence memory (`learn` feature, plan Phase 11).
//!
//! Every record a research run retrieves is remembered with how that run's
//! verification used it: `supported`, `partially_supported`, `contradicted`
//! or `uncited`. A record is keyed by its DOI, else its URL, else its title
//! and source, so the same paper found again updates one row. Recall ranks
//! by BM25 over title and snippet, with the same per-connection `temp` FTS5
//! index as memory recall, so the database file never gains a virtual
//! table. The `evidence_memory` table is created on first use.

use std::collections::HashSet;

use re_core::rank::tokenize;
use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::conn::Store;
use crate::error::StoreError;

const CREATE: &str = "CREATE TABLE IF NOT EXISTS evidence_memory (
    key TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    snippet TEXT NOT NULL,
    source TEXT NOT NULL,
    doi TEXT,
    url TEXT,
    last_run_id TEXT NOT NULL,
    last_status TEXT NOT NULL,
    last_verified_at TEXT NOT NULL,
    times_seen INTEGER NOT NULL DEFAULT 1
)";

/// One remembered record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RememberedEvidence {
    /// Title.
    pub title: String,
    /// Snippet as last retrieved.
    pub snippet: String,
    /// Source adapter.
    pub source: String,
    /// DOI, when known.
    pub doi: Option<String>,
    /// URL, when known.
    pub url: Option<String>,
    /// The last run that retrieved it.
    pub last_run_id: String,
    /// How that run's verification used it.
    pub last_status: String,
    /// When that run verified it.
    pub last_verified_at: String,
    /// Runs that retrieved it.
    pub times_seen: i64,
}

/// The identity of a record across runs.
#[must_use]
pub fn evidence_key(doi: Option<&str>, url: Option<&str>, title: &str, source: &str) -> String {
    if let Some(doi) = doi.map(str::trim).filter(|d| !d.is_empty()) {
        return format!("doi:{}", doi.to_lowercase());
    }
    if let Some(url) = url.map(str::trim).filter(|u| !u.is_empty()) {
        return format!("url:{url}");
    }
    format!("title:{source}:{}", title.trim().to_lowercase())
}

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
    /// Remember `records` from one run (insert, or update the last run,
    /// status and snippet and count the sighting).
    ///
    /// # Errors
    /// `SQLite` failures.
    pub fn remember_evidence(&self, records: &[RememberedEvidence]) -> Result<(), StoreError> {
        self.with_tx(|tx| {
            tx.execute_batch(CREATE)?;
            for r in records {
                let key = evidence_key(r.doi.as_deref(), r.url.as_deref(), &r.title, &r.source);
                tx.execute(
                    "INSERT INTO evidence_memory
                        (key, title, snippet, source, doi, url, last_run_id, last_status,
                         last_verified_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                     ON CONFLICT(key) DO UPDATE SET
                        title = excluded.title, snippet = excluded.snippet,
                        last_run_id = excluded.last_run_id, last_status = excluded.last_status,
                        last_verified_at = excluded.last_verified_at,
                        times_seen = times_seen + (last_run_id != excluded.last_run_id)",
                    params![
                        key,
                        r.title,
                        r.snippet,
                        r.source,
                        r.doi,
                        r.url,
                        r.last_run_id,
                        r.last_status,
                        r.last_verified_at
                    ],
                )?;
            }
            Ok(())
        })
    }

    /// Remembered records matching any word of `query`, best BM25 first.
    ///
    /// # Errors
    /// `SQLite` failures.
    pub fn recall_evidence(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<RememberedEvidence>, StoreError> {
        let Some(expression) = match_expression(query) else {
            return Ok(Vec::new());
        };
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        self.with_tx(|tx| {
            tx.execute_batch(CREATE)?;
            tx.execute_batch(
                "CREATE VIRTUAL TABLE IF NOT EXISTS temp.evidence_memory_fts USING fts5(
                    key UNINDEXED, title, snippet,
                    tokenize = 'unicode61 remove_diacritics 2'
                 );
                 DELETE FROM temp.evidence_memory_fts;
                 INSERT INTO temp.evidence_memory_fts (key, title, snippet)
                    SELECT key, title, snippet FROM evidence_memory;",
            )?;
            let mut statement = tx.prepare(
                "SELECT m.title, m.snippet, m.source, m.doi, m.url, m.last_run_id,
                        m.last_status, m.last_verified_at, m.times_seen
                 FROM temp.evidence_memory_fts(?1) AS f
                 JOIN evidence_memory AS m ON m.key = f.key
                 ORDER BY f.rank, m.last_verified_at DESC
                 LIMIT ?2",
            )?;
            let rows = statement
                .query_map(params![expression, limit], |row| {
                    Ok(RememberedEvidence {
                        title: row.get(0)?,
                        snippet: row.get(1)?,
                        source: row.get(2)?,
                        doi: row.get(3)?,
                        url: row.get(4)?,
                        last_run_id: row.get(5)?,
                        last_status: row.get(6)?,
                        last_verified_at: row.get(7)?,
                        times_seen: row.get(8)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(title: &str, doi: Option<&str>, run: &str, status: &str) -> RememberedEvidence {
        RememberedEvidence {
            title: title.into(),
            snippet: format!("{title} snippet"),
            source: "openalex".into(),
            doi: doi.map(str::to_string),
            url: None,
            last_run_id: run.into(),
            last_status: status.into(),
            last_verified_at: "2026-10-19T00:00:00Z".into(),
            times_seen: 1,
        }
    }

    #[test]
    fn records_are_merged_by_identity_and_recalled_by_relevance() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("m.db")).unwrap();
        assert_eq!(store.recall_evidence("naps", 5).unwrap(), []);
        store
            .remember_evidence(&[
                record(
                    "Daytime naps and power",
                    Some("10.1/A"),
                    "run_1",
                    "supported",
                ),
                record("Protein before sleep", None, "run_1", "uncited"),
            ])
            .unwrap();
        store
            .remember_evidence(&[record(
                "Daytime naps and power",
                Some("10.1/a"),
                "run_2",
                "contradicted",
            )])
            .unwrap();
        let found = store.recall_evidence("daytime naps power", 5).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].last_run_id, "run_2");
        assert_eq!(found[0].last_status, "contradicted");
        assert_eq!(found[0].times_seen, 2, "seen by two runs");
        assert_eq!(
            store.count_rows("evidence_memory").unwrap_or(-1),
            -1,
            "not a core table"
        );
        assert_eq!(
            evidence_key(None, None, " Title ", "dblp"),
            "title:dblp:title"
        );
    }
}
