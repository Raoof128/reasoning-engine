//! Append-only transparency log of exported run packs (plan Phase 7).
//!
//! Each signed export appends `(seq, run_id, run_pack_hash, prev_hash,
//! node_hash)`, where `node_hash = sha256("{prev_hash}\n{run_id}\n
//! {run_pack_hash}")` and the first `prev_hash` is [`GENESIS`]. Rewriting
//! or deleting any earlier row breaks every later `node_hash`, so the head
//! hash commits to the whole history. The plan named a Merkle log; for one
//! local writer a hash chain gives the same tamper evidence with a simpler
//! proof (the chain up to the entry).
//!
//! The table is created on first use, so a database the Python
//! implementation shares never gains it unless signing is on.

use re_core::hashing::sha256_prefixed;
use rusqlite::{OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};

use crate::conn::Store;
use crate::error::StoreError;

/// `prev_hash` of the first entry.
pub const GENESIS: &str = "sha256:0000000000000000000000000000000000000000000000000000000000000000";

const CREATE: &str = "CREATE TABLE IF NOT EXISTS transparency_log (
    seq INTEGER PRIMARY KEY,
    run_id TEXT NOT NULL,
    run_pack_hash TEXT NOT NULL,
    prev_hash TEXT NOT NULL,
    node_hash TEXT NOT NULL UNIQUE,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
)";

/// One log entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogEntry {
    /// Position, from 1.
    pub seq: i64,
    /// The exported run.
    pub run_id: String,
    /// Its attestation's `run_pack_hash`.
    pub run_pack_hash: String,
    /// The previous entry's `node_hash`, or [`GENESIS`].
    pub prev_hash: String,
    /// This entry's hash.
    pub node_hash: String,
}

/// `node_hash` for an entry.
#[must_use]
pub fn node_hash(prev_hash: &str, run_id: &str, run_pack_hash: &str) -> String {
    sha256_prefixed(format!("{prev_hash}\n{run_id}\n{run_pack_hash}").as_bytes())
}

/// Check a chain from the first entry: sequence numbers, links and hashes.
///
/// # Errors
/// A description of the first broken link.
pub fn verify_chain(entries: &[LogEntry]) -> Result<(), String> {
    let mut prev = GENESIS.to_string();
    for (expected_seq, entry) in (1..).zip(entries) {
        if entry.seq != expected_seq {
            return Err(format!("entry {} is out of sequence", entry.seq));
        }
        if entry.prev_hash != prev {
            return Err(format!(
                "entry {} does not link to its predecessor",
                entry.seq
            ));
        }
        if entry.node_hash != node_hash(&entry.prev_hash, &entry.run_id, &entry.run_pack_hash) {
            return Err(format!("entry {} has the wrong hash", entry.seq));
        }
        prev.clone_from(&entry.node_hash);
    }
    Ok(())
}

fn entries(tx: &Transaction<'_>, up_to: i64) -> Result<Vec<LogEntry>, StoreError> {
    let mut stmt = tx.prepare(
        "SELECT seq, run_id, run_pack_hash, prev_hash, node_hash FROM transparency_log
         WHERE seq <= ?1 ORDER BY seq",
    )?;
    let rows = stmt
        .query_map([up_to], |row| {
            Ok(LogEntry {
                seq: row.get(0)?,
                run_id: row.get(1)?,
                run_pack_hash: row.get(2)?,
                prev_hash: row.get(3)?,
                node_hash: row.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

impl Store {
    /// Append an export to the log.
    ///
    /// # Errors
    /// `SQLite` failures.
    pub fn append_transparency(
        &self,
        run_id: &str,
        run_pack_hash: &str,
    ) -> Result<LogEntry, StoreError> {
        self.with_tx(|tx| {
            tx.execute_batch(CREATE)?;
            let last: Option<(i64, String)> = tx
                .query_row(
                    "SELECT seq, node_hash FROM transparency_log ORDER BY seq DESC LIMIT 1",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;
            let (seq, prev_hash) = last.map_or((1, GENESIS.to_string()), |(s, h)| (s + 1, h));
            let entry = LogEntry {
                seq,
                run_id: run_id.into(),
                run_pack_hash: run_pack_hash.into(),
                node_hash: node_hash(&prev_hash, run_id, run_pack_hash),
                prev_hash,
            };
            tx.execute(
                "INSERT INTO transparency_log (seq, run_id, run_pack_hash, prev_hash, node_hash)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    entry.seq,
                    entry.run_id,
                    entry.run_pack_hash,
                    entry.prev_hash,
                    entry.node_hash
                ],
            )?;
            Ok(entry)
        })
    }

    /// The chain from the first entry to the latest entry for `run_id`, or
    /// `None` when the run was never logged (or there is no log).
    ///
    /// # Errors
    /// `SQLite` failures.
    pub fn inclusion_proof(&self, run_id: &str) -> Result<Option<Vec<LogEntry>>, StoreError> {
        self.with_tx(|tx| {
            if !log_exists(tx)? {
                return Ok(None);
            }
            let seq: Option<i64> = tx
                .query_row(
                    "SELECT MAX(seq) FROM transparency_log WHERE run_id = ?1",
                    [run_id],
                    |row| row.get(0),
                )
                .optional()?
                .flatten();
            match seq {
                Some(seq) => Ok(Some(entries(tx, seq)?)),
                None => Ok(None),
            }
        })
    }

    /// Every entry, oldest first (empty when there is no log).
    ///
    /// # Errors
    /// `SQLite` failures.
    pub fn transparency_log(&self) -> Result<Vec<LogEntry>, StoreError> {
        self.with_tx(|tx| {
            if !log_exists(tx)? {
                return Ok(Vec::new());
            }
            entries(tx, i64::MAX)
        })
    }
}

fn log_exists(tx: &Transaction<'_>) -> Result<bool, StoreError> {
    Ok(tx
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'transparency_log'",
            [],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().expect("tmp");
        let store = Store::open(dir.path().join("log.db")).expect("open");
        (dir, store)
    }

    #[test]
    fn entries_chain_and_prove_inclusion() {
        let (_dir, store) = store();
        assert_eq!(store.inclusion_proof("run_a").unwrap(), None);
        assert!(
            !store
                .table_names()
                .unwrap()
                .contains(&"transparency_log".into()),
            "no table until something is logged"
        );
        let a = store.append_transparency("run_a", "sha256:aa").unwrap();
        let b = store.append_transparency("run_b", "sha256:bb").unwrap();
        assert_eq!((a.seq, a.prev_hash.as_str()), (1, GENESIS));
        assert_eq!(b.prev_hash, a.node_hash);
        let proof = store.inclusion_proof("run_b").unwrap().expect("logged");
        assert_eq!(proof, vec![a.clone(), b]);
        assert_eq!(verify_chain(&proof), Ok(()));
        assert_eq!(store.inclusion_proof("run_a").unwrap(), Some(vec![a]));
        assert_eq!(store.inclusion_proof("run_z").unwrap(), None);
    }

    #[test]
    fn rewriting_history_breaks_the_chain() {
        let (_dir, store) = store();
        store.append_transparency("run_a", "sha256:aa").unwrap();
        store.append_transparency("run_b", "sha256:bb").unwrap();
        let mut log = store.transparency_log().unwrap();
        log[0].run_pack_hash = "sha256:forged".into();
        assert_eq!(verify_chain(&log), Err("entry 1 has the wrong hash".into()));
        let mut log = store.transparency_log().unwrap();
        log.remove(0);
        assert!(verify_chain(&log).is_err(), "a deleted entry is detected");
    }
}
