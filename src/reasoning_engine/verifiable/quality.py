"""Report quality gate for final verifiable research outputs."""

from __future__ import annotations

from typing import Any

from reasoning_engine.verifiable.models import utc_now

BLOCKING_STATUSES = {"unsupported", "needs_more_evidence", "contradicted"}


def _field(record: Any, name: str, default: Any = None) -> Any:
    if isinstance(record, dict):
        return record.get(name, default)
    return getattr(record, name, default)


def run_quality_gate(
    run_id: str,
    claims: list[Any],
    verifications: list[Any],
    gaps: list[dict[str, Any]],
) -> dict[str, Any]:
    verification_by_claim = {_field(item, "claim_id"): item for item in verifications}
    blocking_failures: list[str] = []
    warnings: list[str] = []

    unsupported = [
        _field(claim, "claim_id")
        for claim in claims
        if _field(claim, "requires_citation")
        and verification_by_claim.get(_field(claim, "claim_id")) is not None
        and _field(verification_by_claim[_field(claim, "claim_id")], "support_status")
        in BLOCKING_STATUSES
    ]
    missing_verifications = [
        _field(claim, "claim_id")
        for claim in claims
        if _field(claim, "requires_citation")
        and _field(claim, "claim_id") not in verification_by_claim
    ]
    if unsupported or missing_verifications:
        blocking_failures.append("unsupported factual claims remain")
    if gaps:
        warnings.append("unresolved evidence gaps are present")

    result = "blocked" if blocking_failures else ("pass_with_warnings" if warnings else "pass")
    return {
        "gate_id": f"gate_{run_id}",
        "run_id": run_id,
        "result": result,
        "blocking_failures": blocking_failures,
        "warnings": warnings,
        "checked_at": utc_now(),
    }
