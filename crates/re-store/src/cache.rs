//! Storage for the evidence cache (plan Phase 8). The table is created on
//! first use, so a database shared with Python never gains it unless the
//! cache is on.

use rusqlite::{OptionalExtension, params};

use crate::conn::Store;
use crate::error::StoreError;

const CREATE: &str = "CREATE TABLE IF NOT EXISTS evidence_cache (
    key TEXT PRIMARY KEY,
    source TEXT NOT NULL,
    query_norm TEXT NOT NULL,
    payload TEXT NOT NULL,
    fetched_at INTEGER NOT NULL,
    ttl_s INTEGER NOT NULL
)";

impl Store {
    /// The cached payload for `key` when `fetched_at + ttl_s > now`.
    ///
    /// # Errors
    /// `SQLite` failures.
    pub fn cached_evidence(&self, key: &str, now: i64) -> Result<Option<String>, StoreError> {
        self.with_tx(|tx| {
            tx.execute_batch(CREATE)?;
            Ok(tx
                .query_row(
                    "SELECT payload FROM evidence_cache WHERE key = ?1 AND fetched_at + ttl_s > ?2",
                    params![key, now],
                    |row| row.get(0),
                )
                .optional()?)
        })
    }

    /// Store or replace a cached payload, and drop entries that have
    /// expired so the table cannot grow without bound.
    ///
    /// # Errors
    /// `SQLite` failures.
    pub fn cache_evidence(
        &self,
        key: &str,
        source: &str,
        query_norm: &str,
        payload: &str,
        ttl_s: i64,
        now: i64,
    ) -> Result<(), StoreError> {
        self.with_tx(|tx| {
            tx.execute_batch(CREATE)?;
            tx.execute(
                "DELETE FROM evidence_cache WHERE fetched_at + ttl_s <= ?1",
                [now],
            )?;
            tx.execute(
                "INSERT OR REPLACE INTO evidence_cache
                    (key, source, query_norm, payload, fetched_at, ttl_s)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![key, source, query_norm, payload, now, ttl_s],
            )?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_expire_and_are_swept() {
        let dir = tempfile::tempdir().expect("tmp");
        let store = Store::open(dir.path().join("c.db")).expect("open");
        assert_eq!(store.cached_evidence("k", 0).unwrap(), None);
        store
            .cache_evidence("k", "openalex", "q", "[]", 100, 1_000)
            .unwrap();
        assert_eq!(
            store.cached_evidence("k", 1_099).unwrap().as_deref(),
            Some("[]")
        );
        assert_eq!(
            store.cached_evidence("k", 1_100).unwrap(),
            None,
            "expired at the TTL"
        );
        store
            .cache_evidence("other", "arxiv", "q", "[1]", 10, 2_000)
            .unwrap();
        assert_eq!(
            store.count_rows("evidence_cache").unwrap_or(-1),
            -1,
            "not a core table"
        );
        assert_eq!(
            store.cached_evidence("k", 0).unwrap(),
            None,
            "swept on write"
        );
    }
}
