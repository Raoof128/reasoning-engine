//! Verifiable research ledger (port of verifiable/store.py).
//!
//! Payload columns hold `json.dumps(payload, sort_keys=True)` text written
//! by `re_core::pyjson`, so rows are byte-identical to Python's and hashes
//! over them agree.

use re_core::hashing::stable_json_hash;
use re_core::pyjson::{DumpOptions, dumps};
use re_model::clock::utc_now;
use re_model::ids::short_hex;
use re_model::records::{
    ClaimRecord, EvidenceGap, EvidenceRecord, ResearchRun, VerificationRecord,
};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::conn::Store;
use crate::error::StoreError;

/// A `research_runs` row, columns in table order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunRow {
    /// Run ID.
    pub run_id: String,
    /// Research question.
    pub query: String,
    /// Profile name.
    pub profile: String,
    /// Research mode.
    pub mode: String,
    /// Lifecycle state.
    pub status: String,
    /// Creation time.
    pub created_at: String,
}

fn to_value<T: Serialize>(record: &T) -> Value {
    // Records are plain strings, numbers, lists and maps; this cannot fail.
    serde_json::to_value(record).unwrap_or(Value::Null)
}

fn stored(payload: &Value) -> String {
    dumps(payload, &DumpOptions::sorted())
}

impl Store {
    /// Insert or replace a run.
    ///
    /// # Errors
    /// `SQLite` failures.
    pub fn save_run(&self, run: &ResearchRun) -> Result<(), StoreError> {
        self.with_tx(|tx| {
            tx.execute(
                "INSERT OR REPLACE INTO research_runs
                    (run_id, query, profile, mode, status, created_at)
                 VALUES (?, ?, ?, ?, ?, ?)",
                params![
                    run.run_id,
                    run.query,
                    run.profile,
                    run.mode,
                    run.status,
                    run.created_at
                ],
            )?;
            Ok(())
        })
    }

    /// Fetch a run.
    ///
    /// # Errors
    /// `unknown research run: <id>`, or `SQLite` failures.
    pub fn get_run(&self, run_id: &str) -> Result<RunRow, StoreError> {
        self.with_tx(|tx| {
            tx.query_row(
                "SELECT run_id, query, profile, mode, status, created_at FROM research_runs WHERE run_id = ?",
                [run_id],
                |row| {
                    Ok(RunRow {
                        run_id: row.get(0)?,
                        query: row.get(1)?,
                        profile: row.get(2)?,
                        mode: row.get(3)?,
                        status: row.get(4)?,
                        created_at: row.get(5)?,
                    })
                },
            )
            .optional()?
            .ok_or_else(|| StoreError::invalid(format!("unknown research run: {run_id}")))
        })
    }

    /// Insert or replace evidence; stores `to_dict()` including `snippet_hash`.
    ///
    /// # Errors
    /// `SQLite` failures, including an unknown run.
    pub fn save_evidence(&self, evidence: &EvidenceRecord) -> Result<(), StoreError> {
        let payload = evidence.to_dict();
        let snippet_hash = evidence.snippet_hash();
        self.with_tx(|tx| {
            tx.execute(
                "INSERT OR REPLACE INTO evidence_records
                    (evidence_id, run_id, payload, snippet_hash)
                 VALUES (?, ?, ?, ?)",
                params![
                    evidence.evidence_id,
                    evidence.run_id,
                    stored(&payload),
                    snippet_hash
                ],
            )?;
            Ok(())
        })
    }

    /// Insert or replace an evidence gap.
    ///
    /// # Errors
    /// `SQLite` failures, including an unknown run.
    pub fn save_gap(&self, gap: &EvidenceGap) -> Result<(), StoreError> {
        let payload = to_value(gap);
        self.with_tx(|tx| {
            tx.execute(
                "INSERT OR REPLACE INTO evidence_gaps (gap_id, run_id, payload) VALUES (?, ?, ?)",
                params![gap.gap_id, gap.run_id, stored(&payload)],
            )?;
            Ok(())
        })
    }

    /// Insert or replace a claim after re-checking its type and status.
    ///
    /// # Errors
    /// An invalid claim type or status, or `SQLite` failures.
    pub fn save_claim(&self, claim: &ClaimRecord) -> Result<(), StoreError> {
        claim
            .validate()
            .map_err(|e| StoreError::invalid(e.to_string()))?;
        let payload = to_value(claim);
        self.with_tx(|tx| {
            tx.execute(
                "INSERT OR REPLACE INTO claims (claim_id, run_id, payload, status)
                 VALUES (?, ?, ?, ?)",
                params![claim.claim_id, claim.run_id, stored(&payload), claim.status],
            )?;
            Ok(())
        })
    }

    /// Insert or replace a verification after re-checking its status.
    ///
    /// # Errors
    /// An invalid status, or `SQLite` failures including an unknown claim.
    pub fn save_verification(
        &self,
        run_id: &str,
        verification: &VerificationRecord,
    ) -> Result<(), StoreError> {
        verification
            .validate()
            .map_err(|e| StoreError::invalid(e.to_string()))?;
        let payload = to_value(verification);
        self.with_tx(|tx| {
            tx.execute(
                "INSERT OR REPLACE INTO claim_verifications
                    (verification_id, claim_id, run_id, payload, support_status)
                 VALUES (?, ?, ?, ?, ?)",
                params![
                    verification.verification_id,
                    verification.claim_id,
                    run_id,
                    stored(&payload),
                    verification.support_status,
                ],
            )?;
            Ok(())
        })
    }

    /// Append a provenance event.
    ///
    /// The event is `{run_id, event_type, timestamp, **payload}`: payload keys
    /// override the first three, as in Python. The ID is the event's
    /// `stable_json_hash` plus a random suffix, so identical events in the
    /// same second are all kept.
    ///
    /// # Errors
    /// `SQLite` failures, including an unknown run.
    pub fn append_provenance(
        &self,
        run_id: &str,
        event_type: &str,
        payload: &Map<String, Value>,
    ) -> Result<(), StoreError> {
        let mut event = Map::new();
        event.insert("run_id".into(), run_id.into());
        event.insert("event_type".into(), event_type.into());
        event.insert("timestamp".into(), utc_now().into());
        for (key, value) in payload {
            event.insert(key.clone(), value.clone());
        }
        let event = Value::Object(event);
        let event_id = format!("{}:{}", stable_json_hash(&event), short_hex(12));
        self.with_tx(|tx| {
            tx.execute(
                "INSERT OR REPLACE INTO provenance_events (event_id, run_id, event_type, payload)
                 VALUES (?, ?, ?, ?)",
                params![event_id, run_id, event_type, stored(&event)],
            )?;
            Ok(())
        })
    }

    fn list_payloads(
        &self,
        sql: &str,
        run_id: &str,
        column: &'static str,
    ) -> Result<Vec<Value>, StoreError> {
        let texts: Vec<String> = self.with_tx(|tx| {
            let mut stmt = tx.prepare(sql)?;
            let rows = stmt
                .query_map([run_id], |row| row.get(0))?
                .collect::<Result<Vec<String>, _>>()?;
            Ok(rows)
        })?;
        texts
            .iter()
            .map(|t| serde_json::from_str(t).map_err(|source| StoreError::Json { column, source }))
            .collect()
    }

    /// Evidence payloads for a run, by evidence ID.
    ///
    /// # Errors
    /// A corrupt payload, or `SQLite` failures.
    pub fn list_evidence(&self, run_id: &str) -> Result<Vec<Value>, StoreError> {
        self.list_payloads(
            "SELECT payload FROM evidence_records WHERE run_id = ? ORDER BY evidence_id",
            run_id,
            "evidence_records.payload",
        )
    }

    /// Gap payloads for a run, by gap ID.
    ///
    /// # Errors
    /// A corrupt payload, or `SQLite` failures.
    pub fn list_gaps(&self, run_id: &str) -> Result<Vec<Value>, StoreError> {
        self.list_payloads(
            "SELECT payload FROM evidence_gaps WHERE run_id = ? ORDER BY gap_id",
            run_id,
            "evidence_gaps.payload",
        )
    }

    /// Claim payloads for a run, by claim ID.
    ///
    /// # Errors
    /// A corrupt payload, or `SQLite` failures.
    pub fn list_claims(&self, run_id: &str) -> Result<Vec<Value>, StoreError> {
        self.list_payloads(
            "SELECT payload FROM claims WHERE run_id = ? ORDER BY claim_id",
            run_id,
            "claims.payload",
        )
    }

    /// Verification payloads for a run, by verification ID.
    ///
    /// # Errors
    /// A corrupt payload, or `SQLite` failures.
    pub fn list_verifications(&self, run_id: &str) -> Result<Vec<Value>, StoreError> {
        self.list_payloads(
            "SELECT payload FROM claim_verifications WHERE run_id = ? ORDER BY verification_id",
            run_id,
            "claim_verifications.payload",
        )
    }

    /// Provenance events for a run in insertion order. Ordered by `rowid`
    /// because `created_at` has one-second resolution and a pipeline emits
    /// many events per second.
    ///
    /// # Errors
    /// A corrupt payload, or `SQLite` failures.
    pub fn list_provenance(&self, run_id: &str) -> Result<Vec<Value>, StoreError> {
        self.list_payloads(
            "SELECT payload FROM provenance_events WHERE run_id = ? ORDER BY rowid",
            run_id,
            "provenance_events.payload",
        )
    }
}
