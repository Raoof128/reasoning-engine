# Verifiable Research Engine MVP Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the Required MVP for Verifiable Research Engine 10: local STDIO MCP tools, a transport-neutral research service, Scholar Gateway retrieval with mocked default tests, claim/evidence ledgers, quality gates, run packs, and tamper-evident attestation.

**Architecture:** Keep the existing FastMCP server thin. Add a transport-neutral service layer under `src/reasoning_engine/verifiable/` with focused modules for profiles, retrieval, ledgers, quality gates, run-pack export, and attestation. SQLite stores queryable state; exported run packs store audit artifacts; tokens are read from environment or keyring and never persisted.

**Tech Stack:** Python 3.12, FastMCP from `mcp`, SQLite, standard-library `dataclasses`, `json`, `hashlib`, `pathlib`, `urllib.request`, and pytest.

---

## Scope

This plan implements the Required MVP items from the approved spec:

- Local STDIO MCP server tools
- Transport-neutral service layer
- Scholar Gateway adapter interface
- Scholar Gateway live calls gated by environment
- Evidence ledger
- Claim ledger and claim types
- Quality gate
- Run pack export
- Attestation manifest and verification

Phase 2/3 items such as localhost HTTP MCP, verified memory, human review ledger, adversarial benchmarks, and extra retrieval adapters are intentionally excluded from this MVP plan.

## File Structure

- Create: `src/reasoning_engine/verifiable/__init__.py`
  - Public package marker and selected exports.
- Create: `src/reasoning_engine/verifiable/models.py`
  - Dataclasses and enum constants for profiles, runs, evidence, claims, verification records, gaps, quality gates, and attestation.
- Create: `src/reasoning_engine/verifiable/profiles.py`
  - Built-in domain profiles and deterministic profile/mode selection.
- Create: `src/reasoning_engine/verifiable/retrieval.py`
  - Retrieval adapter protocol, Scholar Gateway adapter, mock adapter, typed retrieval errors.
- Create: `src/reasoning_engine/verifiable/store.py`
  - SQLite persistence for research runs, evidence, gaps, claims, verifications, provenance, and attestations.
- Create: `src/reasoning_engine/verifiable/claims.py`
  - Claim extraction, claim typing, evidence linking, and support-status classification.
- Create: `src/reasoning_engine/verifiable/quality.py`
  - Quality gate evaluation and blocking rules.
- Create: `src/reasoning_engine/verifiable/runpack.py`
  - Run-pack folder export, JSON/BibTeX/Markdown artifact writing, hash manifest creation, attestation verification.
- Create: `src/reasoning_engine/verifiable/service.py`
  - Transport-neutral orchestration API used by CLI and MCP tools.
- Create: `src/reasoning_engine/cli.py`
  - Console entrypoint for `reasoning-engine` MVP commands.
- Modify: `src/reasoning_engine/db.py`
  - Add explicit research tables and bump `PRAGMA user_version`.
- Modify: `src/reasoning_engine/server.py`
  - Register thin MCP wrappers for the MVP research tools.
- Modify: `pyproject.toml`
  - Add a `reasoning-engine` console script.
- Create: `tests/test_verifiable_models.py`
- Create: `tests/test_verifiable_profiles.py`
- Create: `tests/test_verifiable_retrieval.py`
- Create: `tests/test_verifiable_store.py`
- Create: `tests/test_verifiable_claims_quality.py`
- Create: `tests/test_verifiable_runpack.py`
- Create: `tests/test_verifiable_service.py`
- Create: `tests/test_cli.py`
- Modify: `tests/test_db.py`
- Modify: `tests/test_server.py`

---

### Task 1: Data Models and Package Boundary

**Files:**
- Create: `src/reasoning_engine/verifiable/__init__.py`
- Create: `src/reasoning_engine/verifiable/models.py`
- Test: `tests/test_verifiable_models.py`

- [ ] **Step 1: Write failing model tests**

Create `tests/test_verifiable_models.py`:

```python
from reasoning_engine.verifiable.models import (
    ClaimRecord,
    EvidenceRecord,
    ResearchRun,
    RetrievalError,
    normalize_status,
)


def test_evidence_record_hashes_snippet():
    evidence = EvidenceRecord(
        evidence_id="ev_0001",
        run_id="run_001",
        source_adapter="scholar_gateway",
        source_type="peer_reviewed_article",
        title="A useful paper",
        authors=["A. Researcher"],
        year=2026,
        publisher="Wiley",
        venue="Journal",
        doi="10.1000/example",
        url="https://doi.org/10.1000/example",
        retrieved_at="2026-06-21T00:00:00Z",
        query="test query",
        rank=1,
        score=0.82,
        snippet="Important evidence.",
        licence_notes=None,
        risk_flags=[],
        metadata={},
    )

    payload = evidence.to_dict()

    assert payload["snippet_hash"].startswith("sha256:")
    assert payload["snippet"] == "Important evidence."


def test_claim_record_defaults_to_needs_more_evidence():
    claim = ClaimRecord(
        claim_id="claim_0001",
        run_id="run_001",
        text="Scholar Gateway returns Wiley article metadata.",
        claim_type="empirical",
        domain="general",
        importance="medium",
        risk_level="low",
        requires_citation=True,
        created_from="draft",
    )

    assert claim.status == "needs_more_evidence"
    assert claim.evidence_ids == []


def test_retrieval_error_serializes_without_token_values():
    error = RetrievalError(
        error_type="auth_required",
        message="Set SCHOLAR_GATEWAY_ACCESS_TOKEN",
        retryable=False,
        metadata={"env_var": "SCHOLAR_GATEWAY_ACCESS_TOKEN"},
    )

    assert error.to_dict() == {
        "error_type": "auth_required",
        "message": "Set SCHOLAR_GATEWAY_ACCESS_TOKEN",
        "retryable": False,
        "metadata": {"env_var": "SCHOLAR_GATEWAY_ACCESS_TOKEN"},
    }


def test_research_run_has_profile_and_mode():
    run = ResearchRun.create(query="Explain MCP security", profile="security", mode="standard")

    assert run.run_id.startswith("run_")
    assert run.query == "Explain MCP security"
    assert run.profile == "security"
    assert run.mode == "standard"


def test_normalize_status_rejects_unknown_status():
    assert normalize_status("supported") == "supported"

    try:
        normalize_status("unknown")
    except ValueError as exc:
        assert "support status" in str(exc)
    else:
        raise AssertionError("unknown status should fail")
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
pytest tests/test_verifiable_models.py -v
```

Expected: FAIL with `ModuleNotFoundError: No module named 'reasoning_engine.verifiable'`.

- [ ] **Step 3: Add model package implementation**

Create `src/reasoning_engine/verifiable/__init__.py`:

```python
"""Verifiable research engine components."""

from reasoning_engine.verifiable.models import (
    ClaimRecord,
    EvidenceRecord,
    ResearchRun,
    RetrievalError,
    VerificationRecord,
)

__all__ = [
    "ClaimRecord",
    "EvidenceRecord",
    "ResearchRun",
    "RetrievalError",
    "VerificationRecord",
]
```

Create `src/reasoning_engine/verifiable/models.py`:

```python
"""Shared data models for verifiable research workflows."""

from __future__ import annotations

import hashlib
import json
import uuid
from dataclasses import asdict, dataclass, field
from datetime import UTC, datetime
from typing import Any

SUPPORT_STATUSES = {
    "supported",
    "partially_supported",
    "contradicted",
    "unsupported",
    "needs_more_evidence",
    "hypothesis",
    "not_verifiable",
    "out_of_scope",
}

CLAIM_TYPES = {
    "empirical",
    "definitional",
    "causal",
    "comparative",
    "quantitative",
    "historical",
    "legal",
    "medical",
    "security",
    "policy",
    "predictive",
    "normative",
    "methodological",
    "hypothesis",
}

RETRIEVAL_ERROR_TYPES = {
    "auth_required",
    "rate_limited",
    "unavailable",
    "malformed_response",
    "empty_result",
    "licence_restricted",
    "unsupported_query",
    "adapter_misconfigured",
}


def utc_now() -> str:
    return datetime.now(UTC).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def stable_json_hash(value: Any) -> str:
    encoded = json.dumps(value, sort_keys=True, separators=(",", ":")).encode()
    return f"sha256:{hashlib.sha256(encoded).hexdigest()}"


def normalize_status(status: str) -> str:
    if status not in SUPPORT_STATUSES:
        raise ValueError(f"unknown support status: {status}")
    return status


def normalize_claim_type(claim_type: str) -> str:
    if claim_type not in CLAIM_TYPES:
        raise ValueError(f"unknown claim type: {claim_type}")
    return claim_type


@dataclass(frozen=True)
class ResearchRun:
    run_id: str
    query: str
    profile: str
    mode: str
    created_at: str
    status: str = "active"

    @classmethod
    def create(cls, query: str, profile: str, mode: str) -> "ResearchRun":
        return cls(
            run_id=f"run_{uuid.uuid4().hex[:12]}",
            query=query,
            profile=profile,
            mode=mode,
            created_at=utc_now(),
        )

    def to_dict(self) -> dict[str, Any]:
        return asdict(self)


@dataclass(frozen=True)
class EvidenceRecord:
    evidence_id: str
    run_id: str
    source_adapter: str
    source_type: str
    title: str
    authors: list[str]
    year: int | None
    publisher: str
    venue: str
    doi: str | None
    url: str | None
    retrieved_at: str
    query: str
    rank: int
    score: float
    snippet: str
    licence_notes: str | None
    risk_flags: list[str]
    metadata: dict[str, Any]

    @property
    def snippet_hash(self) -> str:
        return f"sha256:{hashlib.sha256(self.snippet.encode()).hexdigest()}"

    def to_dict(self) -> dict[str, Any]:
        payload = asdict(self)
        payload["snippet_hash"] = self.snippet_hash
        return payload


@dataclass
class ClaimRecord:
    claim_id: str
    run_id: str
    text: str
    claim_type: str
    domain: str
    importance: str
    risk_level: str
    requires_citation: bool
    created_from: str
    status: str = "needs_more_evidence"
    evidence_ids: list[str] = field(default_factory=list)
    contradiction_ids: list[str] = field(default_factory=list)
    confidence: float | None = None
    human_review_required: bool = False

    def __post_init__(self) -> None:
        normalize_claim_type(self.claim_type)
        normalize_status(self.status)

    def to_dict(self) -> dict[str, Any]:
        return asdict(self)


@dataclass(frozen=True)
class VerificationRecord:
    verification_id: str
    claim_id: str
    method: str
    evidence_ids: list[str]
    support_status: str
    support_rationale: str
    missing_evidence: str | None
    contradictory_evidence_ids: list[str]
    confidence: float
    requires_human_review: bool
    verified_at: str

    def __post_init__(self) -> None:
        normalize_status(self.support_status)

    def to_dict(self) -> dict[str, Any]:
        return asdict(self)


@dataclass(frozen=True)
class EvidenceGap:
    gap_id: str
    run_id: str
    query: str
    reason: str
    created_at: str

    def to_dict(self) -> dict[str, Any]:
        return asdict(self)


@dataclass(frozen=True)
class RetrievalError:
    error_type: str
    message: str
    retryable: bool
    metadata: dict[str, Any]

    def __post_init__(self) -> None:
        if self.error_type not in RETRIEVAL_ERROR_TYPES:
            raise ValueError(f"unknown retrieval error type: {self.error_type}")

    def to_dict(self) -> dict[str, Any]:
        return asdict(self)
```

- [ ] **Step 4: Run model tests**

Run:

```bash
pytest tests/test_verifiable_models.py -v
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/reasoning_engine/verifiable/__init__.py src/reasoning_engine/verifiable/models.py tests/test_verifiable_models.py
git commit -m "feat(research): add verifiable research models"
```

---

### Task 2: Profiles and Mode Selection

**Files:**
- Create: `src/reasoning_engine/verifiable/profiles.py`
- Test: `tests/test_verifiable_profiles.py`

- [ ] **Step 1: Write failing profile tests**

Create `tests/test_verifiable_profiles.py`:

```python
from reasoning_engine.verifiable.profiles import (
    classify_research_mode,
    get_profile,
    list_profiles,
    select_profile,
)


def test_list_profiles_includes_required_profiles():
    names = [profile.name for profile in list_profiles()]

    assert "general" in names
    assert "security" in names
    assert "medicine" in names
    assert "ai_safety" in names


def test_select_profile_uses_security_keywords():
    profile = select_profile("How should MCP tools handle prompt injection?")

    assert profile.name == "security"
    assert profile.claim_strictness == "high"


def test_select_profile_defaults_to_general():
    profile = select_profile("Summarize the history of printing")

    assert profile.name == "general"


def test_classify_mode_escalates_high_stakes_security():
    mode = classify_research_mode("Can this vulnerability leak credentials?", requested_mode="standard")

    assert mode == "high_stakes"


def test_requested_scholarly_mode_is_respected_for_low_risk_query():
    mode = classify_research_mode("Compare literature synthesis methods", requested_mode="scholarly")

    assert mode == "scholarly"


def test_unknown_profile_fails():
    try:
        get_profile("missing")
    except ValueError as exc:
        assert "unknown profile" in str(exc)
    else:
        raise AssertionError("unknown profile should fail")
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
pytest tests/test_verifiable_profiles.py -v
```

Expected: FAIL with `ModuleNotFoundError` or missing functions.

- [ ] **Step 3: Implement profiles**

Create `src/reasoning_engine/verifiable/profiles.py`:

```python
"""Domain profiles and deterministic mode selection."""

from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True)
class ResearchProfile:
    name: str
    preferred_source_types: tuple[str, ...]
    weak_source_types: tuple[str, ...]
    minimum_evidence_count: int
    recency_requirement: str
    citation_style: str
    claim_strictness: str
    high_risk_triggers: tuple[str, ...]
    required_caveats: tuple[str, ...]
    scoring_weights: dict[str, float]


BASE_WEIGHTS = {
    "source_quality": 0.25,
    "citation_support": 0.30,
    "recency": 0.15,
    "contradiction_handling": 0.20,
    "provenance_completeness": 0.10,
}

PROFILES = {
    "general": ResearchProfile(
        name="general",
        preferred_source_types=("peer_reviewed_article", "standards_document", "book", "primary_source"),
        weak_source_types=("social_media",),
        minimum_evidence_count=1,
        recency_requirement="normal",
        citation_style="harvard_au",
        claim_strictness="medium",
        high_risk_triggers=(),
        required_caveats=(),
        scoring_weights=BASE_WEIGHTS,
    ),
    "medicine": ResearchProfile(
        name="medicine",
        preferred_source_types=("peer_reviewed_article", "clinical_guideline", "systematic_review"),
        weak_source_types=("blog_post", "social_media"),
        minimum_evidence_count=2,
        recency_requirement="fast",
        citation_style="vancouver",
        claim_strictness="maximum",
        high_risk_triggers=("diagnosis", "treatment", "drug", "dose", "clinical"),
        required_caveats=("Medical findings require professional clinical review.",),
        scoring_weights=BASE_WEIGHTS,
    ),
    "law": ResearchProfile(
        name="law",
        preferred_source_types=("legislation", "case_law", "regulator_guidance", "peer_reviewed_article"),
        weak_source_types=("blog_post", "social_media"),
        minimum_evidence_count=2,
        recency_requirement="fast",
        citation_style="legal",
        claim_strictness="maximum",
        high_risk_triggers=("liability", "compliance", "contract", "statute", "jurisdiction"),
        required_caveats=("Legal findings require jurisdiction-specific professional review.",),
        scoring_weights=BASE_WEIGHTS,
    ),
    "business": ResearchProfile(
        name="business",
        preferred_source_types=("market_report", "financial_filing", "peer_reviewed_article"),
        weak_source_types=("social_media",),
        minimum_evidence_count=2,
        recency_requirement="normal",
        citation_style="harvard_au",
        claim_strictness="high",
        high_risk_triggers=("investment", "valuation", "forecast", "revenue"),
        required_caveats=("Business and financial conclusions depend on changing market conditions.",),
        scoring_weights=BASE_WEIGHTS,
    ),
    "engineering": ResearchProfile(
        name="engineering",
        preferred_source_types=("standards_document", "technical_report", "peer_reviewed_article"),
        weak_source_types=("social_media",),
        minimum_evidence_count=2,
        recency_requirement="normal",
        citation_style="ieee",
        claim_strictness="high",
        high_risk_triggers=("safety", "failure", "load", "certification"),
        required_caveats=("Engineering claims require context-specific validation.",),
        scoring_weights=BASE_WEIGHTS,
    ),
    "science": ResearchProfile(
        name="science",
        preferred_source_types=("peer_reviewed_article", "preprint", "dataset"),
        weak_source_types=("blog_post", "social_media"),
        minimum_evidence_count=2,
        recency_requirement="normal",
        citation_style="harvard_au",
        claim_strictness="high",
        high_risk_triggers=("causal", "replication", "statistically significant"),
        required_caveats=(),
        scoring_weights=BASE_WEIGHTS,
    ),
    "humanities": ResearchProfile(
        name="humanities",
        preferred_source_types=("book", "peer_reviewed_article", "primary_source"),
        weak_source_types=("social_media",),
        minimum_evidence_count=1,
        recency_requirement="normal",
        citation_style="chicago",
        claim_strictness="medium",
        high_risk_triggers=("attribution", "translation", "archive"),
        required_caveats=(),
        scoring_weights=BASE_WEIGHTS,
    ),
    "policy": ResearchProfile(
        name="policy",
        preferred_source_types=("government_report", "regulator_guidance", "peer_reviewed_article"),
        weak_source_types=("social_media",),
        minimum_evidence_count=2,
        recency_requirement="fast",
        citation_style="harvard_au",
        claim_strictness="high",
        high_risk_triggers=("public health", "national security", "regulation", "election"),
        required_caveats=("Policy conclusions depend on jurisdiction and timing.",),
        scoring_weights=BASE_WEIGHTS,
    ),
    "security": ResearchProfile(
        name="security",
        preferred_source_types=("peer_reviewed_article", "conference_paper", "standards_document", "vendor_advisory"),
        weak_source_types=("blog_post", "social_media"),
        minimum_evidence_count=2,
        recency_requirement="fast",
        citation_style="harvard_au",
        claim_strictness="high",
        high_risk_triggers=("exploit", "vulnerability", "malware", "credential", "prompt injection", "mcp"),
        required_caveats=("Security findings may change quickly as patches and disclosures evolve.",),
        scoring_weights=BASE_WEIGHTS,
    ),
    "ai_safety": ResearchProfile(
        name="ai_safety",
        preferred_source_types=("peer_reviewed_article", "technical_report", "model_card", "standards_document"),
        weak_source_types=("social_media",),
        minimum_evidence_count=2,
        recency_requirement="fast",
        citation_style="harvard_au",
        claim_strictness="high",
        high_risk_triggers=("alignment", "eval", "dangerous capability", "misuse", "agent"),
        required_caveats=("AI safety findings can change quickly as model capabilities and evaluations evolve.",),
        scoring_weights=BASE_WEIGHTS,
    ),
}

PROFILE_KEYWORDS = {
    "security": ("security", "vulnerability", "malware", "credential", "prompt injection", "mcp", "exploit"),
    "ai_safety": ("ai safety", "alignment", "dangerous capability", "eval", "agent"),
    "medicine": ("medical", "clinical", "diagnosis", "treatment", "drug", "patient"),
    "law": ("legal", "law", "contract", "liability", "compliance", "jurisdiction"),
    "business": ("business", "market", "revenue", "investment", "valuation"),
    "engineering": ("engineering", "system design", "safety-critical", "load", "certification"),
    "science": ("science", "experiment", "replication", "dataset", "study"),
    "humanities": ("history", "literature", "archive", "translation", "philosophy"),
    "policy": ("policy", "regulation", "public health", "government", "election"),
}

HIGH_STAKES_TERMS = (
    "medical",
    "clinical",
    "diagnosis",
    "treatment",
    "legal",
    "liability",
    "financial",
    "investment",
    "safety",
    "vulnerability",
    "exploit",
    "credential",
    "public health",
)

VALID_MODES = {"quick", "standard", "deep", "scholarly", "audit", "high_stakes"}


def list_profiles() -> list[ResearchProfile]:
    return [PROFILES[name] for name in sorted(PROFILES)]


def get_profile(name: str) -> ResearchProfile:
    try:
        return PROFILES[name]
    except KeyError as exc:
        raise ValueError(f"unknown profile: {name}") from exc


def select_profile(query: str, requested_profile: str = "auto") -> ResearchProfile:
    if requested_profile != "auto":
        return get_profile(requested_profile)
    lowered = query.lower()
    for profile_name, keywords in PROFILE_KEYWORDS.items():
        if any(keyword in lowered for keyword in keywords):
            return PROFILES[profile_name]
    return PROFILES["general"]


def classify_research_mode(query: str, requested_mode: str = "standard") -> str:
    if requested_mode not in VALID_MODES:
        raise ValueError(f"unknown research mode: {requested_mode}")
    lowered = query.lower()
    if requested_mode == "high_stakes" or any(term in lowered for term in HIGH_STAKES_TERMS):
        return "high_stakes"
    return requested_mode
```

- [ ] **Step 4: Run profile tests**

Run:

```bash
pytest tests/test_verifiable_profiles.py -v
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/reasoning_engine/verifiable/profiles.py tests/test_verifiable_profiles.py
git commit -m "feat(research): add profiles and mode selection"
```

---

### Task 3: SQLite Research Store

**Files:**
- Modify: `src/reasoning_engine/db.py`
- Create: `src/reasoning_engine/verifiable/store.py`
- Modify: `tests/test_db.py`
- Test: `tests/test_verifiable_store.py`

- [ ] **Step 1: Add failing database table tests**

Append to `tests/test_db.py`:

```python

def test_init_db_creates_verifiable_research_tables(db_path):
    conn = sqlite3.connect(db_path)
    cursor = conn.execute("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
    tables = {row[0] for row in cursor.fetchall()}
    version = conn.execute("PRAGMA user_version").fetchone()[0]
    conn.close()

    assert "research_runs" in tables
    assert "evidence_records" in tables
    assert "evidence_gaps" in tables
    assert "claims" in tables
    assert "claim_verifications" in tables
    assert "quality_gate_results" in tables
    assert "provenance_events" in tables
    assert "attestation_manifests" in tables
    assert version >= 2
```

Create `tests/test_verifiable_store.py`:

```python
from reasoning_engine.db import init_db
from reasoning_engine.verifiable.models import ClaimRecord, EvidenceGap, EvidenceRecord, ResearchRun, utc_now
from reasoning_engine.verifiable.store import ResearchStore


def test_store_round_trips_run_evidence_gap_and_claim(db_path):
    init_db(db_path)
    store = ResearchStore(db_path)
    run = ResearchRun.create("Explain Scholar Gateway", profile="general", mode="standard")
    evidence = EvidenceRecord(
        evidence_id="ev_0001",
        run_id=run.run_id,
        source_adapter="scholar_gateway",
        source_type="peer_reviewed_article",
        title="Gateway paper",
        authors=["A. Author"],
        year=2026,
        publisher="Wiley",
        venue="Journal",
        doi="10.1000/example",
        url="https://doi.org/10.1000/example",
        retrieved_at=utc_now(),
        query=run.query,
        rank=1,
        score=0.9,
        snippet="Scholar Gateway exposes semantic search.",
        licence_notes=None,
        risk_flags=[],
        metadata={},
    )
    gap = EvidenceGap(
        gap_id="gap_0001",
        run_id=run.run_id,
        query=run.query,
        reason="No second corroborating source found.",
        created_at=utc_now(),
    )
    claim = ClaimRecord(
        claim_id="claim_0001",
        run_id=run.run_id,
        text="Scholar Gateway exposes semantic search.",
        claim_type="empirical",
        domain="general",
        importance="high",
        risk_level="low",
        requires_citation=True,
        created_from="test",
        evidence_ids=["ev_0001"],
        status="supported",
    )

    store.save_run(run)
    store.save_evidence(evidence)
    store.save_gap(gap)
    store.save_claim(claim)

    assert store.get_run(run.run_id)["query"] == run.query
    assert store.list_evidence(run.run_id)[0]["snippet_hash"].startswith("sha256:")
    assert store.list_gaps(run.run_id)[0]["reason"] == gap.reason
    assert store.list_claims(run.run_id)[0]["status"] == "supported"
```

- [ ] **Step 2: Run store tests to verify they fail**

Run:

```bash
pytest tests/test_db.py::test_init_db_creates_verifiable_research_tables tests/test_verifiable_store.py -v
```

Expected: FAIL because tables and store do not exist.

- [ ] **Step 3: Add research tables to `db.py`**

Modify `init_db()` in `src/reasoning_engine/db.py` by adding these tables inside the existing `conn.executescript(...)` block after the existing indexes:

```sql
            CREATE TABLE IF NOT EXISTS research_runs (
                run_id TEXT PRIMARY KEY,
                query TEXT NOT NULL,
                profile TEXT NOT NULL,
                mode TEXT NOT NULL,
                status TEXT NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS evidence_records (
                evidence_id TEXT PRIMARY KEY,
                run_id TEXT NOT NULL REFERENCES research_runs(run_id),
                payload TEXT NOT NULL,
                snippet_hash TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS evidence_gaps (
                gap_id TEXT PRIMARY KEY,
                run_id TEXT NOT NULL REFERENCES research_runs(run_id),
                payload TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS claims (
                claim_id TEXT PRIMARY KEY,
                run_id TEXT NOT NULL REFERENCES research_runs(run_id),
                payload TEXT NOT NULL,
                status TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS claim_verifications (
                verification_id TEXT PRIMARY KEY,
                claim_id TEXT NOT NULL REFERENCES claims(claim_id),
                run_id TEXT NOT NULL REFERENCES research_runs(run_id),
                payload TEXT NOT NULL,
                support_status TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS quality_gate_results (
                gate_id TEXT PRIMARY KEY,
                run_id TEXT NOT NULL REFERENCES research_runs(run_id),
                payload TEXT NOT NULL,
                result TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS provenance_events (
                event_id TEXT PRIMARY KEY,
                run_id TEXT NOT NULL REFERENCES research_runs(run_id),
                event_type TEXT NOT NULL,
                payload TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS attestation_manifests (
                run_id TEXT PRIMARY KEY REFERENCES research_runs(run_id),
                payload TEXT NOT NULL,
                run_pack_hash TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE INDEX IF NOT EXISTS idx_evidence_run ON evidence_records(run_id);
            CREATE INDEX IF NOT EXISTS idx_gaps_run ON evidence_gaps(run_id);
            CREATE INDEX IF NOT EXISTS idx_claims_run ON claims(run_id);
            CREATE INDEX IF NOT EXISTS idx_verifications_run ON claim_verifications(run_id);
            CREATE INDEX IF NOT EXISTS idx_provenance_run ON provenance_events(run_id);
```

Change:

```python
        conn.execute("PRAGMA user_version = 1")
```

to:

```python
        conn.execute("PRAGMA user_version = 2")
```

- [ ] **Step 4: Implement `ResearchStore`**

Create `src/reasoning_engine/verifiable/store.py`:

```python
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
        event_payload = {"run_id": run_id, "event_type": event_type, "timestamp": utc_now(), **payload}
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
```

- [ ] **Step 5: Run store tests**

Run:

```bash
pytest tests/test_db.py tests/test_verifiable_store.py -v
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/reasoning_engine/db.py src/reasoning_engine/verifiable/store.py tests/test_db.py tests/test_verifiable_store.py
git commit -m "feat(research): persist verifiable research records"
```

---

### Task 4: Scholar Gateway Retrieval Adapter

**Files:**
- Create: `src/reasoning_engine/verifiable/retrieval.py`
- Test: `tests/test_verifiable_retrieval.py`

- [ ] **Step 1: Write failing retrieval tests**

Create `tests/test_verifiable_retrieval.py`:

```python
from reasoning_engine.verifiable.retrieval import (
    MockScholarGatewayAdapter,
    ScholarGatewayAdapter,
)


def test_mock_adapter_returns_normalized_evidence():
    adapter = MockScholarGatewayAdapter()
    result = adapter.search(run_id="run_001", query="MCP prompt injection", limit=2)

    assert result.error is None
    assert len(result.evidence) == 2
    assert result.evidence[0].source_adapter == "scholar_gateway"
    assert result.evidence[0].snippet_hash.startswith("sha256:")


def test_mock_adapter_empty_query_returns_gap_error():
    adapter = MockScholarGatewayAdapter()
    result = adapter.search(run_id="run_001", query="   ", limit=2)

    assert result.evidence == []
    assert result.error is not None
    assert result.error.error_type == "unsupported_query"


def test_live_adapter_requires_token_when_live_enabled(monkeypatch):
    monkeypatch.setenv("SCHOLAR_GATEWAY_LIVE", "1")
    monkeypatch.delenv("SCHOLAR_GATEWAY_ACCESS_TOKEN", raising=False)

    adapter = ScholarGatewayAdapter()
    result = adapter.search(run_id="run_001", query="MCP", limit=1)

    assert result.evidence == []
    assert result.error is not None
    assert result.error.error_type == "auth_required"


def test_live_adapter_uses_mock_when_live_disabled(monkeypatch):
    monkeypatch.delenv("SCHOLAR_GATEWAY_LIVE", raising=False)

    adapter = ScholarGatewayAdapter()
    result = adapter.search(run_id="run_001", query="MCP", limit=1)

    assert result.error is None
    assert len(result.evidence) == 1
```

- [ ] **Step 2: Run retrieval tests to verify they fail**

Run:

```bash
pytest tests/test_verifiable_retrieval.py -v
```

Expected: FAIL because `retrieval.py` does not exist.

- [ ] **Step 3: Implement retrieval adapters**

Create `src/reasoning_engine/verifiable/retrieval.py`:

```python
"""Retrieval adapters for scholarly evidence."""

from __future__ import annotations

import json
import os
import urllib.error
import urllib.request
from dataclasses import dataclass
from typing import Protocol

from reasoning_engine.verifiable.models import EvidenceRecord, RetrievalError, utc_now

SCHOLAR_GATEWAY_MCP_URL = "https://connector.scholargateway.ai/mcp"


@dataclass(frozen=True)
class RetrievalResult:
    evidence: list[EvidenceRecord]
    error: RetrievalError | None = None


class RetrievalAdapter(Protocol):
    def search(self, run_id: str, query: str, limit: int = 10) -> RetrievalResult:
        raise NotImplementedError


class MockScholarGatewayAdapter:
    def search(self, run_id: str, query: str, limit: int = 10) -> RetrievalResult:
        normalized_query = query.strip()
        if not normalized_query:
            return RetrievalResult(
                evidence=[],
                error=RetrievalError(
                    "unsupported_query",
                    "query must not be empty",
                    retryable=False,
                    metadata={},
                ),
            )
        evidence = [
            EvidenceRecord(
                evidence_id=f"ev_{index:04d}",
                run_id=run_id,
                source_adapter="scholar_gateway",
                source_type="peer_reviewed_article",
                title=f"Mock Scholar Gateway Result {index}",
                authors=["Mock Author"],
                year=2026,
                publisher="Wiley",
                venue="Mock Journal",
                doi=f"10.1000/mock.{index}",
                url=f"https://doi.org/10.1000/mock.{index}",
                retrieved_at=utc_now(),
                query=normalized_query,
                rank=index,
                score=max(0.0, 1.0 - (index - 1) * 0.05),
                snippet=f"Mock evidence for {normalized_query}.",
                licence_notes="Mock fixture for tests.",
                risk_flags=[],
                metadata={"mock": True},
            )
            for index in range(1, limit + 1)
        ]
        return RetrievalResult(evidence=evidence)


class ScholarGatewayAdapter:
    def __init__(self, endpoint: str = SCHOLAR_GATEWAY_MCP_URL):
        self.endpoint = endpoint
        self.mock_adapter = MockScholarGatewayAdapter()

    def search(self, run_id: str, query: str, limit: int = 10) -> RetrievalResult:
        if os.environ.get("SCHOLAR_GATEWAY_LIVE") != "1":
            return self.mock_adapter.search(run_id, query, limit)

        token = os.environ.get("SCHOLAR_GATEWAY_ACCESS_TOKEN")
        if not token:
            return RetrievalResult(
                evidence=[],
                error=RetrievalError(
                    "auth_required",
                    "Set SCHOLAR_GATEWAY_ACCESS_TOKEN or complete Scholar Gateway OAuth setup.",
                    retryable=False,
                    metadata={"env_var": "SCHOLAR_GATEWAY_ACCESS_TOKEN"},
                ),
            )

        payload = {
            "jsonrpc": "2.0",
            "id": "semantic_search",
            "method": "tools/call",
            "params": {
                "name": "semantic_search",
                "arguments": {"query": query, "limit": limit},
            },
        }
        request = urllib.request.Request(
            self.endpoint,
            data=json.dumps(payload).encode(),
            headers={
                "Authorization": f"Bearer {token}",
                "Content-Type": "application/json",
                "Accept": "application/json",
            },
            method="POST",
        )
        try:
            with urllib.request.urlopen(request, timeout=30) as response:
                raw = json.loads(response.read().decode())
        except urllib.error.HTTPError as exc:
            if exc.code == 401:
                error_type = "auth_required"
                retryable = False
            elif exc.code == 429:
                error_type = "rate_limited"
                retryable = True
            else:
                error_type = "unavailable"
                retryable = True
            return RetrievalResult(
                evidence=[],
                error=RetrievalError(error_type, f"Scholar Gateway HTTP {exc.code}", retryable, {}),
            )
        except (OSError, TimeoutError, json.JSONDecodeError) as exc:
            return RetrievalResult(
                evidence=[],
                error=RetrievalError("unavailable", str(exc), retryable=True, metadata={}),
            )

        try:
            items = raw.get("result", {}).get("content", [])
            evidence = [
                EvidenceRecord(
                    evidence_id=f"ev_{index:04d}",
                    run_id=run_id,
                    source_adapter="scholar_gateway",
                    source_type="peer_reviewed_article",
                    title=str(item.get("title", "Untitled")),
                    authors=list(item.get("authors", [])),
                    year=item.get("year"),
                    publisher=str(item.get("publisher", "Wiley")),
                    venue=str(item.get("venue", "")),
                    doi=item.get("doi"),
                    url=item.get("url"),
                    retrieved_at=utc_now(),
                    query=query,
                    rank=index,
                    score=float(item.get("score", 0.0)),
                    snippet=str(item.get("snippet", "")),
                    licence_notes=item.get("licence_notes"),
                    risk_flags=[],
                    metadata={"raw": item},
                )
                for index, item in enumerate(items, start=1)
            ]
        except (TypeError, ValueError) as exc:
            return RetrievalResult(
                evidence=[],
                error=RetrievalError("malformed_response", str(exc), retryable=False, metadata={}),
            )

        if not evidence:
            return RetrievalResult(
                evidence=[],
                error=RetrievalError("empty_result", "Scholar Gateway returned no results.", False, {}),
            )
        return RetrievalResult(evidence=evidence)
```

- [ ] **Step 4: Run retrieval tests**

Run:

```bash
pytest tests/test_verifiable_retrieval.py -v
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/reasoning_engine/verifiable/retrieval.py tests/test_verifiable_retrieval.py
git commit -m "feat(research): add scholar gateway retrieval adapter"
```

---

### Task 5: Claim Verification and Quality Gate

**Files:**
- Create: `src/reasoning_engine/verifiable/claims.py`
- Create: `src/reasoning_engine/verifiable/quality.py`
- Test: `tests/test_verifiable_claims_quality.py`

- [ ] **Step 1: Write failing claim and quality tests**

Create `tests/test_verifiable_claims_quality.py`:

```python
from reasoning_engine.verifiable.claims import extract_claims, verify_claims
from reasoning_engine.verifiable.models import EvidenceRecord, utc_now
from reasoning_engine.verifiable.quality import run_quality_gate


def _evidence(snippet: str) -> EvidenceRecord:
    return EvidenceRecord(
        evidence_id="ev_0001",
        run_id="run_001",
        source_adapter="scholar_gateway",
        source_type="peer_reviewed_article",
        title="Evidence",
        authors=["A. Author"],
        year=2026,
        publisher="Wiley",
        venue="Journal",
        doi="10.1000/example",
        url="https://doi.org/10.1000/example",
        retrieved_at=utc_now(),
        query="query",
        rank=1,
        score=0.9,
        snippet=snippet,
        licence_notes=None,
        risk_flags=[],
        metadata={},
    )


def test_extract_claims_splits_sentences_and_marks_quantitative():
    claims = extract_claims(
        run_id="run_001",
        text="Scholar Gateway has 8 million articles. This is a good direction.",
        domain="general",
    )

    assert len(claims) == 2
    assert claims[0].claim_type == "quantitative"
    assert claims[0].requires_citation is True


def test_verify_claims_supports_claim_when_evidence_contains_terms():
    claims = extract_claims("run_001", "Scholar Gateway exposes semantic search.", "general")
    verifications = verify_claims(
        claims,
        [_evidence("Scholar Gateway exposes semantic search for Wiley articles.")],
    )

    assert verifications[0].support_status == "supported"
    assert verifications[0].evidence_ids == ["ev_0001"]


def test_verify_claims_marks_missing_evidence():
    claims = extract_claims("run_001", "Scholar Gateway exposes semantic search.", "general")
    verifications = verify_claims(claims, [])

    assert verifications[0].support_status == "needs_more_evidence"
    assert verifications[0].missing_evidence


def test_quality_gate_blocks_unsupported_final_claims():
    claims = extract_claims("run_001", "Scholar Gateway exposes semantic search.", "general")
    verifications = verify_claims(claims, [])
    gate = run_quality_gate(run_id="run_001", claims=claims, verifications=verifications, gaps=[])

    assert gate["result"] == "blocked"
    assert "unsupported factual claims remain" in gate["blocking_failures"]


def test_quality_gate_passes_supported_claims():
    claims = extract_claims("run_001", "Scholar Gateway exposes semantic search.", "general")
    verifications = verify_claims(
        claims,
        [_evidence("Scholar Gateway exposes semantic search for Wiley articles.")],
    )
    gate = run_quality_gate(run_id="run_001", claims=claims, verifications=verifications, gaps=[])

    assert gate["result"] == "pass"
    assert gate["blocking_failures"] == []
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
pytest tests/test_verifiable_claims_quality.py -v
```

Expected: FAIL because claim and quality modules do not exist.

- [ ] **Step 3: Implement claim extraction and verification**

Create `src/reasoning_engine/verifiable/claims.py`:

```python
"""Claim extraction and evidence-based verification."""

from __future__ import annotations

import re
import uuid

from reasoning_engine.verifiable.models import ClaimRecord, EvidenceRecord, VerificationRecord, utc_now

WORD_RE = re.compile(r"[a-z0-9]+")


def _claim_type(text: str) -> str:
    lowered = text.lower()
    if re.search(r"\d", text):
        return "quantitative"
    if any(term in lowered for term in ("causes", "caused", "leads to", "because")):
        return "causal"
    if any(term in lowered for term in ("should", "must", "ought")):
        return "normative"
    if any(term in lowered for term in ("may", "might", "could", "likely")):
        return "hypothesis"
    return "empirical"


def extract_claims(run_id: str, text: str, domain: str) -> list[ClaimRecord]:
    sentences = [part.strip() for part in re.split(r"(?<=[.!?])\s+", text.strip()) if part.strip()]
    claims: list[ClaimRecord] = []
    for index, sentence in enumerate(sentences, start=1):
        claim_type = _claim_type(sentence)
        claims.append(
            ClaimRecord(
                claim_id=f"claim_{index:04d}",
                run_id=run_id,
                text=sentence.rstrip(".!?"),
                claim_type=claim_type,
                domain=domain,
                importance="high" if claim_type in {"quantitative", "causal"} else "medium",
                risk_level="medium" if domain in {"security", "medicine", "law"} else "low",
                requires_citation=claim_type not in {"normative", "hypothesis"},
                created_from="draft",
            )
        )
    return claims


def _overlap_score(claim_text: str, snippet: str) -> float:
    claim_terms = {term for term in WORD_RE.findall(claim_text.lower()) if len(term) > 3}
    snippet_terms = {term for term in WORD_RE.findall(snippet.lower()) if len(term) > 3}
    if not claim_terms:
        return 0.0
    return len(claim_terms & snippet_terms) / len(claim_terms)


def verify_claims(
    claims: list[ClaimRecord],
    evidence: list[EvidenceRecord],
) -> list[VerificationRecord]:
    verifications: list[VerificationRecord] = []
    for claim in claims:
        scored = sorted(
            ((_overlap_score(claim.text, item.snippet), item) for item in evidence),
            key=lambda pair: pair[0],
            reverse=True,
        )
        best_score, best_evidence = scored[0] if scored else (0.0, None)
        if not claim.requires_citation:
            status = "hypothesis" if claim.claim_type == "hypothesis" else "not_verifiable"
            evidence_ids: list[str] = []
            rationale = "Claim does not require factual citation support."
            missing = None
        elif best_evidence is None:
            status = "needs_more_evidence"
            evidence_ids = []
            rationale = "No evidence was available for this claim."
            missing = "Retrieve source evidence that directly supports the claim."
        elif best_score >= 0.65:
            status = "supported"
            evidence_ids = [best_evidence.evidence_id]
            rationale = "Evidence snippet overlaps the claim strongly enough for MVP support."
            missing = None
        elif best_score >= 0.35:
            status = "partially_supported"
            evidence_ids = [best_evidence.evidence_id]
            rationale = "Evidence is relevant but does not support the exact claim."
            missing = "Find direct evidence for the full claim."
        else:
            status = "unsupported"
            evidence_ids = []
            rationale = "Available evidence does not support the claim."
            missing = "Find direct evidence or remove the claim from final mode."

        claim.status = status
        claim.evidence_ids = evidence_ids
        claim.confidence = round(best_score, 2)
        verifications.append(
            VerificationRecord(
                verification_id=f"ver_{uuid.uuid4().hex[:12]}",
                claim_id=claim.claim_id,
                method="term_overlap_mvp",
                evidence_ids=evidence_ids,
                support_status=status,
                support_rationale=rationale,
                missing_evidence=missing,
                contradictory_evidence_ids=[],
                confidence=round(best_score, 2),
                requires_human_review=status in {"partially_supported", "unsupported", "needs_more_evidence"},
                verified_at=utc_now(),
            )
        )
    return verifications
```

- [ ] **Step 4: Implement quality gate**

Create `src/reasoning_engine/verifiable/quality.py`:

```python
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
```

- [ ] **Step 5: Run claim and quality tests**

Run:

```bash
pytest tests/test_verifiable_claims_quality.py -v
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/reasoning_engine/verifiable/claims.py src/reasoning_engine/verifiable/quality.py tests/test_verifiable_claims_quality.py
git commit -m "feat(research): verify claims and gate report quality"
```

---

### Task 6: Run Pack Export and Attestation

**Files:**
- Create: `src/reasoning_engine/verifiable/runpack.py`
- Test: `tests/test_verifiable_runpack.py`

- [ ] **Step 1: Write failing run-pack tests**

Create `tests/test_verifiable_runpack.py`:

```python
import json
from pathlib import Path

from reasoning_engine.verifiable.models import ClaimRecord, EvidenceRecord, ResearchRun, utc_now
from reasoning_engine.verifiable.runpack import export_run_pack, verify_run_pack_attestation


def _evidence(run_id: str) -> EvidenceRecord:
    return EvidenceRecord(
        evidence_id="ev_0001",
        run_id=run_id,
        source_adapter="scholar_gateway",
        source_type="peer_reviewed_article",
        title="Evidence",
        authors=["A. Author"],
        year=2026,
        publisher="Wiley",
        venue="Journal",
        doi="10.1000/example",
        url="https://doi.org/10.1000/example",
        retrieved_at=utc_now(),
        query="query",
        rank=1,
        score=0.9,
        snippet="Scholar Gateway exposes semantic search.",
        licence_notes=None,
        risk_flags=[],
        metadata={},
    )


def test_export_run_pack_writes_required_artifacts(tmp_path):
    run = ResearchRun.create("Explain Scholar Gateway", "general", "standard")
    claim = ClaimRecord(
        claim_id="claim_0001",
        run_id=run.run_id,
        text="Scholar Gateway exposes semantic search",
        claim_type="empirical",
        domain="general",
        importance="high",
        risk_level="low",
        requires_citation=True,
        created_from="test",
        status="supported",
        evidence_ids=["ev_0001"],
    )

    output = export_run_pack(
        base_dir=tmp_path,
        run=run,
        evidence=[_evidence(run.run_id).to_dict()],
        gaps=[],
        claims=[claim.to_dict()],
        verifications=[],
        provenance=[],
        quality_gate={"result": "pass", "blocking_failures": [], "warnings": []},
        report_markdown="# Report\n\nSupported report.",
    )

    assert (output / "run.json").exists()
    assert (output / "evidence_ledger.json").exists()
    assert (output / "claims.json").exists()
    assert (output / "attestation.json").exists()
    assert verify_run_pack_attestation(output)["valid"] is True


def test_tampered_run_pack_fails_verification(tmp_path):
    run = ResearchRun.create("Explain Scholar Gateway", "general", "standard")
    output = export_run_pack(
        base_dir=tmp_path,
        run=run,
        evidence=[],
        gaps=[],
        claims=[],
        verifications=[],
        provenance=[],
        quality_gate={"result": "pass", "blocking_failures": [], "warnings": []},
        report_markdown="# Report\n",
    )
    Path(output / "report.md").write_text("# Tampered\n", encoding="utf-8")

    result = verify_run_pack_attestation(output)

    assert result["valid"] is False
    assert "report.md" in result["mismatched_artifacts"]
```

- [ ] **Step 2: Run run-pack tests to verify they fail**

Run:

```bash
pytest tests/test_verifiable_runpack.py -v
```

Expected: FAIL because `runpack.py` does not exist.

- [ ] **Step 3: Implement run-pack export and attestation**

Create `src/reasoning_engine/verifiable/runpack.py`:

```python
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
        entries.append(f"@article{{{key},\n  title = {{{title}}},\n  year = {{{year}}},\n  doi = {{{doi}}}\n}}")
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
        [
            {"claim_id": claim["claim_id"], "evidence_ids": claim.get("evidence_ids", [])}
            for claim in claims
        ],
    )
    _write_json(output / "claim_verifications.json", verifications)
    _write_json(output / "quality_gate.json", quality_gate)
    (output / "report.md").write_text(report_markdown, encoding="utf-8")
    (output / "unresolved_gaps.md").write_text(
        "\n".join(f"- {gap.get('reason', 'Unresolved evidence gap')}" for gap in gaps) + ("\n" if gaps else ""),
        encoding="utf-8",
    )
    (output / "sources.bib").write_text(_source_bib(evidence), encoding="utf-8")
    (output / "methodology.md").write_text(
        "# Methodology\n\nEvidence was retrieved through configured adapters, claims were verified against the same evidence records exported in the run pack, and the quality gate was run before export.\n\nMVP verification uses deterministic lexical overlap as a placeholder verifier. It is suitable for pipeline testing, not final semantic claim verification.\n",
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
        return {"valid": False, "mismatched_artifacts": ["attestation.json"], "missing_artifacts": ["attestation.json"]}
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
```

- [ ] **Step 4: Run run-pack tests**

Run:

```bash
pytest tests/test_verifiable_runpack.py -v
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/reasoning_engine/verifiable/runpack.py tests/test_verifiable_runpack.py
git commit -m "feat(research): export attested run packs"
```

---

### Task 7: Transport-Neutral Service

**Files:**
- Create: `src/reasoning_engine/verifiable/service.py`
- Test: `tests/test_verifiable_service.py`

- [ ] **Step 1: Write failing service tests**

Create `tests/test_verifiable_service.py`:

```python
from reasoning_engine.db import init_db
from reasoning_engine.verifiable.service import VerifiableResearchService


def test_service_runs_pipeline_and_blocks_unsupported_claim(tmp_path, db_path):
    init_db(db_path)
    service = VerifiableResearchService(db_path=db_path, runs_dir=tmp_path)

    result = service.run_research_pipeline(
        query="A claim with no matching evidence about unrelated material.",
        draft="A claim with no matching evidence about unrelated material.",
        mode="standard",
        profile="general",
        use_mock_empty=True,
    )

    assert result["quality_gate"]["result"] == "blocked"
    assert result["run_id"].startswith("run_")


def test_service_runs_pipeline_and_exports_attested_pack(tmp_path, db_path):
    init_db(db_path)
    service = VerifiableResearchService(db_path=db_path, runs_dir=tmp_path)

    result = service.run_research_pipeline(
        query="Scholar Gateway exposes semantic search",
        draft="Scholar Gateway exposes semantic search.",
        mode="standard",
        profile="general",
    )

    assert result["quality_gate"]["result"] == "pass"
    assert result["run_pack"]
    assert result["attestation"]["valid"] is True
    assert result["evidence"][0]["snippet"] in result["verified_against_snippets"]
```

- [ ] **Step 2: Run service tests to verify they fail**

Run:

```bash
pytest tests/test_verifiable_service.py -v
```

Expected: FAIL because `service.py` does not exist.

- [ ] **Step 3: Implement service orchestration**

Create `src/reasoning_engine/verifiable/service.py`:

```python
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
        self.store.append_provenance(run.run_id, "profile_selected", {"profile": selected_profile.name})
        self.store.append_provenance(run.run_id, "mode_selected", {"mode": selected_mode})
        return run

    def scholar_search(self, run_id: str, query: str, limit: int = 10, use_mock_empty: bool = False) -> dict[str, Any]:
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

    def _build_report(self, run: ResearchRun, claims: list[Any], verifications: list[Any], gate: dict[str, Any]) -> str:
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
```

- [ ] **Step 4: Run service tests**

Run:

```bash
pytest tests/test_verifiable_service.py -v
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/reasoning_engine/verifiable/service.py tests/test_verifiable_service.py
git commit -m "feat(research): orchestrate verifiable research pipeline"
```

---

### Task 8: MCP Tool Surface

**Files:**
- Modify: `src/reasoning_engine/server.py`
- Modify: `tests/test_server.py`

- [ ] **Step 1: Write failing MCP wrapper tests**

Modify the import block in `tests/test_server.py` to include:

```python
    classify_research_mode_tool,
    export_run_pack_tool,
    get_scholar_auth_status,
    run_quality_gate_tool,
    run_research_pipeline_tool,
    scholar_search_tool,
    start_research_run,
```

Append:

```python

def test_verifiable_start_research_run_tool():
    data = json.loads(start_research_run("Explain MCP prompt injection", mode="standard", profile="auto"))

    assert data["run_id"].startswith("run_")
    assert data["profile"] == "security"
    assert data["mode"] == "high_stakes"


def test_verifiable_scholar_search_tool_uses_mock_by_default():
    run = json.loads(start_research_run("Scholar Gateway semantic search"))
    data = json.loads(scholar_search_tool(run["run_id"], "Scholar Gateway semantic search", limit=1))

    assert data["evidence"][0]["source_adapter"] == "scholar_gateway"
    assert data["error"] is None


def test_get_scholar_auth_status_does_not_expose_token(monkeypatch):
    monkeypatch.setenv("SCHOLAR_GATEWAY_ACCESS_TOKEN", "secret-token")
    data = json.loads(get_scholar_auth_status())

    assert data["has_env_token"] is True
    assert "secret-token" not in json.dumps(data)


def test_run_research_pipeline_tool_exports_pack():
    data = json.loads(
        run_research_pipeline_tool(
            "Scholar Gateway exposes semantic search",
            "Scholar Gateway exposes semantic search.",
            mode="standard",
            profile="general",
        )
    )

    assert data["run_id"].startswith("run_")
    assert data["attestation"]["valid"] is True
```

- [ ] **Step 2: Run MCP tests to verify they fail**

Run:

```bash
pytest tests/test_server.py -v
```

Expected: FAIL because new MCP wrapper functions do not exist.

- [ ] **Step 3: Add MCP wrappers**

In `src/reasoning_engine/server.py`, add imports:

```python
from reasoning_engine.verifiable.profiles import classify_research_mode
from reasoning_engine.verifiable.quality import run_quality_gate
from reasoning_engine.verifiable.runpack import verify_run_pack_attestation
from reasoning_engine.verifiable.service import VerifiableResearchService
```

After `DB_PATH` add:

```python
RUNS_DIR = os.environ.get(
    "REASONING_ENGINE_RUNS_DIR",
    os.path.join(os.path.dirname(os.path.dirname(os.path.dirname(__file__))), "runs"),
)
```

After `evidence_gap_questions_tool`, add:

```python
def _research_service() -> VerifiableResearchService:
    return VerifiableResearchService(DB_PATH, RUNS_DIR)


@mcp.tool()
def start_research_run(query: str, mode: str = "standard", profile: str = "auto") -> str:
    """Create a verifiable research run with selected mode and profile."""
    query = validate_text(query, "query", MAX_QUERY_CHARS)
    run = _research_service().start_run(query=query, mode=mode, profile=profile)
    return _json_response(run.to_dict())


@mcp.tool()
def classify_research_mode_tool(query: str, requested_mode: str = "standard") -> str:
    """Classify the research mode, escalating high-stakes queries."""
    query = validate_text(query, "query", MAX_QUERY_CHARS)
    return _json_response({"mode": classify_research_mode(query, requested_mode)})


@mcp.tool()
def scholar_search_tool(run_id: str, query: str, limit: int = 10) -> str:
    """Search Scholar Gateway through the verifiable research service."""
    run_id = validate_text(run_id, "run_id", 128)
    query = validate_text(query, "query", MAX_QUERY_CHARS)
    limit = validate_limited_int(limit, "limit", 1, 25)
    return _json_response(_research_service().scholar_search(run_id, query, limit))


@mcp.tool()
def get_scholar_auth_status() -> str:
    """Return Scholar Gateway auth status without exposing token values."""
    return _json_response(
        {
            "live_enabled": os.environ.get("SCHOLAR_GATEWAY_LIVE") == "1",
            "has_env_token": bool(os.environ.get("SCHOLAR_GATEWAY_ACCESS_TOKEN")),
            "token_storage": "environment_or_keyring",
        }
    )


@mcp.tool()
def run_research_pipeline_tool(
    query: str,
    draft: str,
    mode: str = "standard",
    profile: str = "auto",
) -> str:
    """Run the MVP verifiable research pipeline and export an attested run pack."""
    query = validate_text(query, "query", MAX_QUERY_CHARS)
    draft = validate_text(draft, "draft", MAX_QUERY_CHARS * 4)
    return _json_response(
        _research_service().run_research_pipeline(query=query, draft=draft, mode=mode, profile=profile)
    )


@mcp.tool()
def run_quality_gate_tool(run_id: str) -> str:
    """Run the quality gate for persisted claims and verifications."""
    run_id = validate_text(run_id, "run_id", 128)
    store = _research_service().store
    claims = store.list_claims(run_id)
    verifications = store.list_verifications(run_id)
    gaps = store.list_gaps(run_id)
    result = run_quality_gate(run_id, claims, verifications, gaps)
    return _json_response(result)


@mcp.tool()
def export_run_pack_tool(query: str, draft: str, mode: str = "standard", profile: str = "auto") -> str:
    """Run the pipeline and return the exported run-pack path."""
    return run_research_pipeline_tool(query, draft, mode, profile)
```

- [ ] **Step 4: Run MCP tests**

Run:

```bash
pytest tests/test_server.py -v
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/reasoning_engine/server.py tests/test_server.py
git commit -m "feat(research): expose verifiable research mcp tools"
```

---

### Task 9: CLI Surface

**Files:**
- Create: `src/reasoning_engine/cli.py`
- Modify: `pyproject.toml`
- Test: `tests/test_cli.py`

- [ ] **Step 1: Write failing CLI tests**

Create `tests/test_cli.py`:

```python
import json

from reasoning_engine.cli import main


def test_cli_scholar_search_outputs_json(tmp_path, db_path, capsys, monkeypatch):
    monkeypatch.setenv("REASONING_ENGINE_DB", db_path)
    monkeypatch.setenv("REASONING_ENGINE_RUNS_DIR", str(tmp_path))

    code = main(["scholar", "search", "Scholar Gateway semantic search", "--limit", "1"])
    captured = capsys.readouterr()
    payload = json.loads(captured.out)

    assert code == 0
    assert payload["evidence"][0]["source_adapter"] == "scholar_gateway"


def test_cli_research_exports_run_pack(tmp_path, db_path, capsys, monkeypatch):
    monkeypatch.setenv("REASONING_ENGINE_DB", db_path)
    monkeypatch.setenv("REASONING_ENGINE_RUNS_DIR", str(tmp_path))

    code = main(
        [
            "research",
            "Scholar Gateway exposes semantic search",
            "--draft",
            "Scholar Gateway exposes semantic search.",
        ]
    )
    captured = capsys.readouterr()
    payload = json.loads(captured.out)

    assert code == 0
    assert payload["attestation"]["valid"] is True
```

- [ ] **Step 2: Run CLI tests to verify they fail**

Run:

```bash
pytest tests/test_cli.py -v
```

Expected: FAIL because `cli.py` does not exist.

- [ ] **Step 3: Implement CLI**

Create `src/reasoning_engine/cli.py`:

```python
"""Command-line interface for reasoning-engine."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path

from reasoning_engine.db import init_db
from reasoning_engine.verifiable.service import VerifiableResearchService


def _service() -> VerifiableResearchService:
    db_path = os.environ.get("REASONING_ENGINE_DB")
    runs_dir = os.environ.get("REASONING_ENGINE_RUNS_DIR", "runs")
    initialized = init_db(db_path)
    return VerifiableResearchService(initialized, Path(runs_dir))


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="reasoning-engine")
    subparsers = parser.add_subparsers(dest="command", required=True)

    research = subparsers.add_parser("research")
    research.add_argument("query")
    research.add_argument("--draft", required=True)
    research.add_argument("--mode", default="standard")
    research.add_argument("--profile", default="auto")

    scholar = subparsers.add_parser("scholar")
    scholar_sub = scholar.add_subparsers(dest="scholar_command", required=True)
    scholar_search = scholar_sub.add_parser("search")
    scholar_search.add_argument("query")
    scholar_search.add_argument("--limit", type=int, default=10)

    return parser


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    service = _service()

    if args.command == "research":
        payload = service.run_research_pipeline(
            query=args.query,
            draft=args.draft,
            mode=args.mode,
            profile=args.profile,
        )
    elif args.command == "scholar" and args.scholar_command == "search":
        run = service.start_run(args.query)
        payload = service.scholar_search(run.run_id, args.query, args.limit)
    else:
        parser.error("unsupported command")

    print(json.dumps(payload, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
```

Modify `pyproject.toml` by adding:

```toml
[project.scripts]
reasoning-engine = "reasoning_engine.cli:main"
```

- [ ] **Step 4: Run CLI tests**

Run:

```bash
pytest tests/test_cli.py -v
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/reasoning_engine/cli.py pyproject.toml tests/test_cli.py
git commit -m "feat(research): add verifiable research cli"
```

---

### Task 10: End-to-End Verification and Documentation Touch-Up

**Files:**
- Modify: `README.md`
- Modify: `docs/api-reference.md`
- Modify: `docs/examples.md`
- Create: `tests/test_verifiable_invariants.py`
- Test: all test files

- [ ] **Step 1: Add invariant tests**

Create `tests/test_verifiable_invariants.py`:

```python
import json
from pathlib import Path

from reasoning_engine.db import init_db
from reasoning_engine.server import get_scholar_auth_status
from reasoning_engine.verifiable.claims import extract_claims, verify_claims
from reasoning_engine.verifiable.quality import run_quality_gate
from reasoning_engine.verifiable.runpack import verify_run_pack_attestation
from reasoning_engine.verifiable.service import VerifiableResearchService


def test_failed_retrieval_creates_no_evidence(tmp_path, db_path):
    init_db(db_path)
    service = VerifiableResearchService(db_path=db_path, runs_dir=tmp_path)

    result = service.run_research_pipeline(
        query="unsupported retrieval path",
        draft="Unsupported retrieval path has evidence.",
        profile="general",
        use_mock_empty=True,
    )

    assert result["evidence"] == []


def test_unsupported_factual_claim_blocks_final_report():
    claims = extract_claims("run_001", "Scholar Gateway exposes semantic search.", "general")
    verifications = verify_claims(claims, [])

    gate = run_quality_gate("run_001", claims, verifications, gaps=[])

    assert gate["result"] == "blocked"


def test_token_value_never_appears_in_auth_status(monkeypatch):
    monkeypatch.setenv("SCHOLAR_GATEWAY_ACCESS_TOKEN", "secret-token-value")

    payload = json.loads(get_scholar_auth_status())

    assert payload["has_env_token"] is True
    assert "secret-token-value" not in json.dumps(payload)


def test_tampered_run_pack_fails_verification(tmp_path, db_path):
    init_db(db_path)
    service = VerifiableResearchService(db_path=db_path, runs_dir=tmp_path)
    result = service.run_research_pipeline(
        query="Scholar Gateway exposes semantic search",
        draft="Scholar Gateway exposes semantic search.",
        profile="general",
    )
    report = Path(result["run_pack"]) / "report.md"
    report.write_text("# Tampered\n", encoding="utf-8")

    assert verify_run_pack_attestation(result["run_pack"])["valid"] is False


def test_final_report_has_run_id_and_quality_gate(tmp_path, db_path):
    init_db(db_path)
    service = VerifiableResearchService(db_path=db_path, runs_dir=tmp_path)
    result = service.run_research_pipeline(
        query="Scholar Gateway exposes semantic search",
        draft="Scholar Gateway exposes semantic search.",
        profile="general",
    )

    report = Path(result["run_pack"]) / "report.md"
    text = report.read_text(encoding="utf-8")

    assert result["run_id"] in text
    assert "Quality gate:" in text
```

- [ ] **Step 2: Run invariant tests**

Run:

```bash
pytest tests/test_verifiable_invariants.py -v
```

Expected: PASS.

- [ ] **Step 3: Add README usage section**

Append this section to `README.md`:

````markdown

## Verifiable Research Engine MVP

Run a local verifiable research pipeline:

```bash
reasoning-engine research "Scholar Gateway exposes semantic search" \
  --draft "Scholar Gateway exposes semantic search."
```

Run a Scholar Gateway search with mocked default retrieval:

```bash
reasoning-engine scholar search "MCP prompt injection" --limit 3
```

Live Scholar Gateway calls are opt-in:

```bash
export SCHOLAR_GATEWAY_LIVE=1
export SCHOLAR_GATEWAY_ACCESS_TOKEN="<token>"
reasoning-engine scholar search "literature synthesis evaluation" --limit 5
```

Tokens are read from environment or local credential mechanisms. Tokens are not
stored in SQLite or run packs. MVP verification uses deterministic lexical
overlap as a placeholder verifier, so it is suitable for pipeline testing and
audit workflow validation rather than final semantic claim verification.
````

- [ ] **Step 4: Add API reference section**

Append this section to `docs/api-reference.md`:

```markdown

## Verifiable Research MCP Tools

- `start_research_run(query, mode="standard", profile="auto")`: creates a research run.
- `classify_research_mode_tool(query, requested_mode="standard")`: classifies or escalates mode.
- `scholar_search_tool(run_id, query, limit=10)`: retrieves normalized Scholar Gateway evidence.
- `get_scholar_auth_status()`: reports live-token availability without exposing token values.
- `run_research_pipeline_tool(query, draft, mode="standard", profile="auto")`: runs retrieval, claim extraction, verification, quality gate, run-pack export, and attestation verification.
- `run_quality_gate_tool(run_id)`: evaluates persisted claims and verifications.
- `export_run_pack_tool(query, draft, mode="standard", profile="auto")`: exports an attested run pack through the pipeline.
```

- [ ] **Step 5: Add example run output**

Append this section to `docs/examples.md`:

````markdown

## Verifiable Research Run

```bash
reasoning-engine research "Scholar Gateway exposes semantic search" \
  --draft "Scholar Gateway exposes semantic search."
```

Expected output includes:

```json
{
  "attestation": {
    "valid": true
  },
  "quality_gate": {
    "result": "pass"
  },
  "run_id": "run_<id>",
  "run_pack": "runs/run_<id>"
}
```
````

- [ ] **Step 6: Run full verification**

Run:

```bash
ruff check .
ruff format --check .
pytest -q
```

Expected: all commands pass.

- [ ] **Step 7: Run CLI smoke manually**

Run:

```bash
REASONING_ENGINE_RUNS_DIR="$(mktemp -d)" reasoning-engine research \
  "Scholar Gateway exposes semantic search" \
  --draft "Scholar Gateway exposes semantic search."
```

Expected: JSON output includes `"valid": true` and a `run_pack` path.

- [ ] **Step 8: Commit**

```bash
git add README.md docs/api-reference.md docs/examples.md tests/test_verifiable_invariants.py
git commit -m "docs(research): document verifiable research mvp"
```

---

## Self-Review

Spec coverage:

- Core invariants are covered by Tasks 3, 4, 5, 6, 7, 8, and 9.
- Local STDIO MCP is covered by Task 8 through existing FastMCP server exports.
- Transport-neutral service layer is covered by Task 7.
- Scholar Gateway adapter and live/mock boundary are covered by Task 4.
- Evidence ledger, claim ledger, and provenance persistence are covered by Task 3.
- Claim extraction, support statuses, and final-report blockers are covered by Task 5.
- Run-pack export and attestation are covered by Task 6.
- CLI surface for `research` and `scholar search` is covered by Task 9.
- Core invariant regression tests, documentation, and full verification are covered by Task 10.

Known exclusions from this MVP plan:

- Localhost Streamable HTTP MCP server.
- Human review ledger.
- Verified research memory.
- Benchmark smoke suite and regression diff.
- Contradiction graph beyond support-status scaffolding.
- Additional retrieval adapters.

These exclusions match the MVP boundary stated at the top of this plan.
