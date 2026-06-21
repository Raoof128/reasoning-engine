"""Transport-neutral verifiable research service."""

from __future__ import annotations

from pathlib import Path
from typing import Any

from reasoning_engine.verifiable.claims import extract_claims, verify_claims
from reasoning_engine.verifiable.models import EvidenceGap, ResearchRun, utc_now
from reasoning_engine.verifiable.profiles import classify_research_mode, select_profile
from reasoning_engine.verifiable.quality import run_quality_gate
from reasoning_engine.verifiable.retrieval import MockScholarGatewayAdapter, ScholarGatewayAdapter
from reasoning_engine.verifiable.runpack import export_run_pack, verify_run_pack_attestation
from reasoning_engine.verifiable.store import ResearchStore


class EmptyMockAdapter(MockScholarGatewayAdapter):
    def search(self, run_id: str, query: str, limit: int = 10):
        from reasoning_engine.verifiable.retrieval import RetrievalResult

        return RetrievalResult(evidence=[])


class VerifiableResearchService:
    def __init__(self, db_path: str, runs_dir: str | Path = "runs"):
        self.store = ResearchStore(db_path)
        self.runs_dir = Path(runs_dir)

    def start_run(self, query: str, mode: str = "standard", profile: str = "auto") -> ResearchRun:
        selected_profile = select_profile(query, profile)
        selected_mode = classify_research_mode(query, mode)
        run = ResearchRun.create(query=query, profile=selected_profile.name, mode=selected_mode)
        self.store.save_run(run)
        self.store.append_provenance(run.run_id, "run_created", {"query": query})
        self.store.append_provenance(
            run.run_id,
            "profile_selected",
            {"profile": selected_profile.name},
        )
        self.store.append_provenance(run.run_id, "mode_selected", {"mode": selected_mode})
        return run

    def scholar_search(
        self,
        run_id: str,
        query: str,
        limit: int = 10,
        use_mock_empty: bool = False,
    ) -> dict[str, Any]:
        adapter = EmptyMockAdapter() if use_mock_empty else ScholarGatewayAdapter()
        self.store.append_provenance(run_id, "scholar_search_requested", {"query": query, "limit": limit})
        result = adapter.search(run_id=run_id, query=query, limit=limit)
        if result.error is not None:
            gap = EvidenceGap(
                gap_id=f"gap_{run_id}",
                run_id=run_id,
                query=query,
                reason=result.error.message,
                created_at=utc_now(),
            )
            self.store.save_gap(gap)
            self.store.append_provenance(run_id, "evidence_gap_recorded", result.error.to_dict())
            return {
                "evidence_records": [],
                "evidence": [],
                "error": result.error.to_dict(),
                "gaps": [gap.to_dict()],
            }
        if not result.evidence:
            gap = EvidenceGap(
                gap_id=f"gap_{run_id}",
                run_id=run_id,
                query=query,
                reason="No evidence found.",
                created_at=utc_now(),
            )
            self.store.save_gap(gap)
            return {"evidence_records": [], "evidence": [], "error": None, "gaps": [gap.to_dict()]}
        for item in result.evidence:
            self.store.save_evidence(item)
        self.store.append_provenance(
            run_id,
            "scholar_search_completed",
            {"evidence_count": len(result.evidence)},
        )
        return {
            "evidence_records": result.evidence,
            "evidence": [item.to_dict() for item in result.evidence],
            "error": None,
            "gaps": [],
        }

    def run_research_pipeline(
        self,
        query: str,
        draft: str,
        mode: str = "standard",
        profile: str = "auto",
        use_mock_empty: bool = False,
    ) -> dict[str, Any]:
        run = self.start_run(query=query, mode=mode, profile=profile)
        search_result = self.scholar_search(run.run_id, query, use_mock_empty=use_mock_empty)
        evidence_dicts = search_result["evidence"]
        evidence = search_result["evidence_records"]
        claims = extract_claims(run.run_id, draft, run.profile)
        verifications = verify_claims(claims, evidence)
        for claim in claims:
            self.store.save_claim(claim)
        for verification in verifications:
            self.store.save_verification(run.run_id, verification)
        gaps = self.store.list_gaps(run.run_id)
        gate = run_quality_gate(run.run_id, claims, verifications, gaps)
        provenance = self.store.list_provenance(run.run_id)
        report = self._build_report(run, claims, verifications, gate)
        output = export_run_pack(
            base_dir=self.runs_dir,
            run=run,
            evidence=evidence_dicts,
            gaps=gaps,
            claims=[claim.to_dict() for claim in claims],
            verifications=[verification.to_dict() for verification in verifications],
            provenance=provenance,
            quality_gate=gate,
            report_markdown=report,
        )
        attestation = verify_run_pack_attestation(output)
        return {
            "run_id": run.run_id,
            "quality_gate": gate,
            "run_pack": str(output),
            "attestation": attestation,
            "evidence": evidence_dicts,
            "verified_against_snippets": [item.snippet for item in evidence],
        }

    def _build_report(
        self,
        run: ResearchRun,
        claims: list[Any],
        verifications: list[Any],
        gate: dict[str, Any],
    ) -> str:
        lines = [
            f"# Research Report {run.run_id}",
            "",
            f"Query: {run.query}",
            f"Profile: {run.profile}",
            f"Mode: {run.mode}",
            f"Quality gate: {gate['result']}",
            "",
            "## Claims",
        ]
        verification_by_claim = {item.claim_id: item for item in verifications}
        for claim in claims:
            verification = verification_by_claim.get(claim.claim_id)
            status = verification.support_status if verification else claim.status
            lines.append(f"- [{status}] {claim.text}")
        return "\n".join(lines) + "\n"
