"""SQLite persistence for verifiable research records."""

from __future__ import annotations

import json
from typing import Any

from reasoning_engine.db import get_connection
from reasoning_engine.verifiable.models import (
    ClaimRecord,
    EvidenceGap,
    EvidenceRecord,
    ResearchRun,
    VerificationRecord,
    stable_json_hash,
    utc_now,
)


class ResearchStore:
    def __init__(self, db_path: str):
        self.db_path = db_path

    def save_run(self, run: ResearchRun) -> None:
        with get_connection(self.db_path) as conn:
            conn.execute(
                """
                INSERT OR REPLACE INTO research_runs
                    (run_id, query, profile, mode, status, created_at)
                VALUES (?, ?, ?, ?, ?, ?)
                """,
                (run.run_id, run.query, run.profile, run.mode, run.status, run.created_at),
            )

    def get_run(self, run_id: str) -> dict[str, Any]:
        with get_connection(self.db_path) as conn:
            row = conn.execute("SELECT * FROM research_runs WHERE run_id = ?", (run_id,)).fetchone()
        if row is None:
            raise ValueError(f"unknown research run: {run_id}")
        return dict(row)

    def save_evidence(self, evidence: EvidenceRecord) -> None:
        payload = evidence.to_dict()
        with get_connection(self.db_path) as conn:
            conn.execute(
                """
                INSERT OR REPLACE INTO evidence_records
                    (evidence_id, run_id, payload, snippet_hash)
                VALUES (?, ?, ?, ?)
                """,
                (
                    evidence.evidence_id,
                    evidence.run_id,
                    json.dumps(payload, sort_keys=True),
                    payload["snippet_hash"],
                ),
            )

    def list_evidence(self, run_id: str) -> list[dict[str, Any]]:
        with get_connection(self.db_path) as conn:
            rows = conn.execute(
                "SELECT payload FROM evidence_records WHERE run_id = ? ORDER BY evidence_id",
                (run_id,),
            ).fetchall()
        return [json.loads(row["payload"]) for row in rows]

    def save_gap(self, gap: EvidenceGap) -> None:
        payload = gap.to_dict()
        with get_connection(self.db_path) as conn:
            conn.execute(
                "INSERT OR REPLACE INTO evidence_gaps (gap_id, run_id, payload) VALUES (?, ?, ?)",
                (gap.gap_id, gap.run_id, json.dumps(payload, sort_keys=True)),
            )

    def list_gaps(self, run_id: str) -> list[dict[str, Any]]:
        with get_connection(self.db_path) as conn:
            rows = conn.execute(
                "SELECT payload FROM evidence_gaps WHERE run_id = ? ORDER BY gap_id",
                (run_id,),
            ).fetchall()
        return [json.loads(row["payload"]) for row in rows]

    def save_claim(self, claim: ClaimRecord) -> None:
        payload = claim.to_dict()
        with get_connection(self.db_path) as conn:
            conn.execute(
                """
                INSERT OR REPLACE INTO claims (claim_id, run_id, payload, status)
                VALUES (?, ?, ?, ?)
                """,
                (claim.claim_id, claim.run_id, json.dumps(payload, sort_keys=True), claim.status),
            )

    def list_claims(self, run_id: str) -> list[dict[str, Any]]:
        with get_connection(self.db_path) as conn:
            rows = conn.execute(
                "SELECT payload FROM claims WHERE run_id = ? ORDER BY claim_id",
                (run_id,),
            ).fetchall()
        return [json.loads(row["payload"]) for row in rows]

    def save_verification(self, run_id: str, verification: VerificationRecord) -> None:
        payload = verification.to_dict()
        with get_connection(self.db_path) as conn:
            conn.execute(
                """
                INSERT OR REPLACE INTO claim_verifications
                    (verification_id, claim_id, run_id, payload, support_status)
                VALUES (?, ?, ?, ?, ?)
                """,
                (
                    verification.verification_id,
                    verification.claim_id,
                    run_id,
                    json.dumps(payload, sort_keys=True),
                    verification.support_status,
                ),
            )

    def list_verifications(self, run_id: str) -> list[dict[str, Any]]:
        with get_connection(self.db_path) as conn:
            rows = conn.execute(
                "SELECT payload FROM claim_verifications WHERE run_id = ? ORDER BY verification_id",
                (run_id,),
            ).fetchall()
        return [json.loads(row["payload"]) for row in rows]

    def append_provenance(self, run_id: str, event_type: str, payload: dict[str, Any]) -> None:
        event_payload = {
            "run_id": run_id,
            "event_type": event_type,
            "timestamp": utc_now(),
            **payload,
        }
        event_id = stable_json_hash(event_payload)
        with get_connection(self.db_path) as conn:
            conn.execute(
                """
                INSERT OR REPLACE INTO provenance_events (event_id, run_id, event_type, payload)
                VALUES (?, ?, ?, ?)
                """,
                (event_id, run_id, event_type, json.dumps(event_payload, sort_keys=True)),
            )

    def list_provenance(self, run_id: str) -> list[dict[str, Any]]:
        with get_connection(self.db_path) as conn:
            rows = conn.execute(
                "SELECT payload FROM provenance_events WHERE run_id = ? ORDER BY created_at, event_id",
                (run_id,),
            ).fetchall()
        return [json.loads(row["payload"]) for row in rows]
