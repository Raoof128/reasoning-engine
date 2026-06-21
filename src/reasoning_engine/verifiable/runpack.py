"""Run-pack export and tamper-evident attestation."""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
from typing import Any

from reasoning_engine.verifiable.models import ResearchRun, stable_json_hash, utc_now

ARTIFACTS = (
    "run.json",
    "provenance.jsonl",
    "evidence_ledger.json",
    "evidence_gaps.json",
    "claims.json",
    "claim_evidence_links.json",
    "claim_verifications.json",
    "quality_gate.json",
    "report.md",
    "unresolved_gaps.md",
    "sources.bib",
    "methodology.md",
)


def _write_json(path: Path, payload: Any) -> None:
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def _hash_file(path: Path) -> str:
    return f"sha256:{hashlib.sha256(path.read_bytes()).hexdigest()}"


def _source_bib(evidence: list[dict[str, Any]]) -> str:
    entries = []
    for item in evidence:
        key = item["evidence_id"]
        title = item.get("title", "")
        year = item.get("year") or ""
        doi = item.get("doi") or ""
        entries.append(
            f"@article{{{key},\n  title = {{{title}}},\n  year = {{{year}}},\n  doi = {{{doi}}}\n}}"
        )
    return "\n\n".join(entries) + ("\n" if entries else "")


def export_run_pack(
    base_dir: str | Path,
    run: ResearchRun,
    evidence: list[dict[str, Any]],
    gaps: list[dict[str, Any]],
    claims: list[dict[str, Any]],
    verifications: list[dict[str, Any]],
    provenance: list[dict[str, Any]],
    quality_gate: dict[str, Any],
    report_markdown: str,
) -> Path:
    output = Path(base_dir) / run.run_id
    output.mkdir(parents=True, exist_ok=True)

    _write_json(output / "run.json", run.to_dict())
    (output / "provenance.jsonl").write_text(
        "".join(json.dumps(event, sort_keys=True) + "\n" for event in provenance),
        encoding="utf-8",
    )
    _write_json(output / "evidence_ledger.json", evidence)
    _write_json(output / "evidence_gaps.json", gaps)
    _write_json(output / "claims.json", claims)
    _write_json(
        output / "claim_evidence_links.json",
        [{"claim_id": claim["claim_id"], "evidence_ids": claim.get("evidence_ids", [])} for claim in claims],
    )
    _write_json(output / "claim_verifications.json", verifications)
    _write_json(output / "quality_gate.json", quality_gate)
    (output / "report.md").write_text(report_markdown, encoding="utf-8")
    (output / "unresolved_gaps.md").write_text(
        "\n".join(f"- {gap.get('reason', 'Unresolved evidence gap')}" for gap in gaps)
        + ("\n" if gaps else ""),
        encoding="utf-8",
    )
    (output / "sources.bib").write_text(_source_bib(evidence), encoding="utf-8")
    (output / "methodology.md").write_text(
        "# Methodology\n\n"
        "Evidence was retrieved through configured adapters, claims were verified against the same "
        "evidence records exported in the run pack, and the quality gate was run before export.\n\n"
        "MVP verification uses deterministic lexical overlap as a placeholder verifier. It is "
        "suitable for pipeline testing, not final semantic claim verification.\n",
        encoding="utf-8",
    )

    artifact_hashes = {name: _hash_file(output / name) for name in ARTIFACTS}
    attestation = {
        "run_id": run.run_id,
        "created_at": utc_now(),
        "engine_version": "0.1.0",
        "artifact_hashes": artifact_hashes,
        "run_pack_hash": stable_json_hash(artifact_hashes),
        "signature": None,
        "signature_algorithm": None,
    }
    _write_json(output / "attestation.json", attestation)
    return output


def verify_run_pack_attestation(run_pack_dir: str | Path) -> dict[str, Any]:
    path = Path(run_pack_dir)
    attestation_path = path / "attestation.json"
    if not attestation_path.exists():
        return {
            "valid": False,
            "mismatched_artifacts": ["attestation.json"],
            "missing_artifacts": ["attestation.json"],
        }
    attestation = json.loads(attestation_path.read_text(encoding="utf-8"))
    mismatched = []
    missing = []
    for name, expected_hash in attestation["artifact_hashes"].items():
        artifact = path / name
        if not artifact.exists():
            missing.append(name)
        elif _hash_file(artifact) != expected_hash:
            mismatched.append(name)
    return {
        "valid": not mismatched and not missing,
        "mismatched_artifacts": mismatched,
        "missing_artifacts": missing,
        "run_pack_hash": attestation.get("run_pack_hash"),
    }
