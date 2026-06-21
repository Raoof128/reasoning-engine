# Verifiable Research Engine 10 Design

Date: 2026-06-21
Status: Proposed
Primary audience: local agentic research workflows for Codex, Claude Code, and
compatible MCP clients
Primary interfaces: CLI, local STDIO MCP, localhost Streamable HTTP MCP
First evidence adapter: Scholar Gateway

## Executive Summary

Verifiable Research Engine 10 upgrades `reasoning-engine` from a hardened
research-orchestration MCP into a local-first, auditable, claim-verifying
research system.

The product thesis is not "another deep research tool." The gap is local-first
research reports with claim-level verification, evidence provenance, quality
gates, and tamper-evident run packs.

The system must produce:

- claim-level evidence links
- machine-readable provenance
- reproducible run packs
- quality-gated reports
- explicit unresolved evidence gaps
- benchmarkable research quality metrics
- tamper-evident artifact manifests
- local-first storage and execution

The first implementation focuses on local workflows for Codex and Claude Code
through MCP. The core remains transport-neutral so future hosted connectors can
be added without rewriting the research engine.

## Approved Direction

Build a transport-neutral core with three interfaces:

1. **Local STDIO MCP**: default and first-class interface for Codex and Claude
   Code.
2. **Localhost Streamable HTTP MCP**: optional local server mode started by the
   user from the terminal, bound to `127.0.0.1` by default.
3. **Future hosted remote MCP**: out of scope for the first implementation, but
   the core should not prevent a later hosted connector.

No general web crawling is in scope. Scholar Gateway is the first trusted
scholarly retrieval adapter, not the entire evidence universe.

## Source Context

The design is based on current platform and research constraints:

- MCP supports STDIO and Streamable HTTP transports. STDIO should receive
  credentials through local environment or local credential mechanisms, while
  HTTP-based MCP can use OAuth-style authorization flows.
- Codex exposes MCP server configuration through `config.toml`, including STDIO
  commands, args, environment variables, HTTP URLs, bearer-token environment
  variables, OAuth scopes, tool allow/deny lists, approval modes, and timeouts.
- Claude Code supports local STDIO and remote HTTP MCP servers, supports OAuth
  for HTTP servers, warns about prompt-injection risk from external-content
  servers, and treats SSE as deprecated in favor of HTTP where available.
- Scholar Gateway exposes an MCP endpoint at
  `https://connector.scholargateway.ai/mcp`, uses OAuth 2.1 via CONNECT SSO,
  and currently documents `semantic_search`.
- Scholar Gateway provides Wiley peer-reviewed journal articles, citation
  metadata, DOI links, text-only content, and semantic-similarity ranking.
- Deep research evaluation literature supports claim-level verification,
  citation support scoring, citation recall/precision, coverage, coherence,
  factual correctness, and separate measurement of retrieval, generation,
  safety, and system behavior.
- MCP security research highlights prompt injection, tool poisoning, weak
  validation, authentication risk, and the need for trace-based auditing.

## Design Principles

### Local First

The default workflow runs on the user's own machine. Artifacts are stored
locally. External calls are explicit and adapter-scoped.

### Transport-Neutral Core

Research logic must not depend on MCP transport objects. CLI, STDIO MCP, and
HTTP MCP call the same service layer.

### Claims Before Prose

Final report prose is downstream of structured claims, evidence, verification
state, and quality-gate results.

### Verifiability Over Fluency

A fluent unsupported paragraph is a failure. A cautious report with explicit
gaps is a success.

### Evidence Adapters, Not Source Lock-In

Scholar Gateway is the first trusted scholarly adapter. The core accepts
normalized evidence records so future adapters can be added safely.

### Tamper-Evident Artifacts

Run packs include artifact hashes and a run-pack hash. Later phases can add
local signatures.

### Untrusted Content Boundary

External text is evidence only. It cannot issue instructions to the agent, tool
runtime, or research engine.

## Core Invariants

These rules are non-negotiable across CLI, STDIO MCP, and localhost HTTP MCP:

1. No unsupported factual claim enters final report mode.
2. No failed retrieval creates evidence.
3. No token is stored in SQLite or run packs.
4. Every exported report has a run ID.
5. Every final claim links to evidence, a gap, a contradiction, or a hypothesis
   status.
6. Tampered run packs fail verification.
7. External content and MCP tool metadata are always treated as untrusted.
8. Public HTTP binding requires explicit unsafe opt-in.

## Architecture

```text
                         CLI / MCP clients
                               |
        +----------------------+----------------------+
        |                      |                      |
  CLI commands           STDIO MCP server       Localhost HTTP MCP
        |                      |                      |
        +----------------------+----------------------+
                               |
                      Research service layer
                               |
        +----------------------+----------------------+
        |                      |                      |
 Retrieval adapters      Verification pipeline    Artifact pipeline
        |                      |                      |
 Scholar Gateway         claim extractor          provenance writer
 future adapters         evidence linker          run pack exporter
 local/manual import     contradiction mapper     attestation writer
                         quality gate             benchmark outputs
```

The service layer is the stable internal API. It creates runs, selects profiles
and modes, dispatches retrieval, normalizes evidence, extracts and verifies
claims, runs quality gates, exports run packs, records provenance, manages
verified memory, and runs benchmarks.

## Example End-to-End Flow

```text
User query
  -> run created
  -> mode and profile selected
  -> research plan created
  -> Scholar Gateway search requested
  -> evidence registered or evidence gaps recorded
  -> claims extracted
  -> claims linked to evidence
  -> contradictions detected
  -> claims verified
  -> quality gate run
  -> report exported
  -> attestation manifest created
  -> run pack verification available
```

Failure branches are explicit:

- failed retrieval creates an adapter error and evidence gap, not evidence
- contradicted evidence creates a contradiction record, not a false conclusion
- unsupported factual claims block final report mode unless downgraded to a
  labeled hypothesis through review

## MVP and Later Scope

| Feature | Scope | Notes |
| --- | --- | --- |
| Local STDIO MCP server | Required MVP | Primary Codex and Claude Code workflow. |
| Transport-neutral service layer | Required MVP | CLI and MCP tools must share this layer. |
| Scholar Gateway adapter interface | Required MVP | Includes mocked tests and typed retrieval errors. |
| Scholar Gateway live calls | Required MVP | Live tests remain opt-in with `SCHOLAR_GATEWAY_LIVE=1`. |
| Evidence ledger | Required MVP | Required before claim verification can be trusted. |
| Claim ledger and claim types | Required MVP | Required for final-report blocking rules. |
| Quality gate | Required MVP | Blocks unsupported factual claims from final mode. |
| Run pack export | Required MVP | Minimum audit artifact. |
| Attestation manifest and verification | Required MVP | Tampered run packs must fail verification. |
| Localhost Streamable HTTP MCP | Phase 2 | Local-only default, optional bearer token. |
| Contradiction graph | Phase 2 | Required for contested evidence workflows. |
| Human review ledger | Phase 2 | Required for high-stakes overrides. |
| Verified memory | Phase 2 | Must include expiry and revalidation. |
| Benchmark smoke suite | Phase 2 | Minimum regression signal. |
| Adversarial and high-stakes benchmarks | Phase 3 | Adds prompt-injection, citation-mismatch, and risk tests. |
| Regression diff command | Phase 3 | Compares benchmark runs across versions. |
| Additional retrieval adapters | Future | arXiv, Crossref, Semantic Scholar, OpenAlex, local PDF. |
| Local signing keys | Future | Ed25519 or equivalent detached signatures. |
| Hosted remote MCP connector | Future | Not part of local-first implementation. |
| Browser dashboard | Future | Not needed for the initial professional CLI/MCP workflow. |

## Retrieval Adapter Layer

The retrieval layer exposes source-specific systems through a common adapter
interface.

### Initial Adapter

The first adapter is Scholar Gateway. Use it for scholarly search when
available, especially for peer-reviewed literature and citation-rich academic
evidence.

### Future Adapter Slots

The architecture reserves adapter slots for:

- Scholar Gateway
- arXiv
- Crossref
- Semantic Scholar
- OpenAlex
- local PDF corpus
- local Markdown corpus
- manual evidence import
- institutional repositories
- specialist domain adapters

These are not implemented in the first phase, but the evidence model must
support them.

### EvidenceRecord

All adapters return the same internal shape:

```json
{
  "evidence_id": "ev_0001",
  "source_adapter": "scholar_gateway",
  "source_type": "peer_reviewed_article",
  "title": "string",
  "authors": ["string"],
  "year": 2026,
  "publisher": "string",
  "venue": "string",
  "doi": "string|null",
  "url": "string|null",
  "retrieved_at": "ISO-8601",
  "query": "string",
  "rank": 1,
  "score": 0.82,
  "snippet": "string",
  "snippet_hash": "sha256",
  "licence_notes": "string|null",
  "risk_flags": [],
  "metadata": {}
}
```

### Retrieval Error Types

Adapters must return typed errors and must never generate fake evidence:

- `auth_required`
- `rate_limited`
- `unavailable`
- `malformed_response`
- `empty_result`
- `licence_restricted`
- `unsupported_query`
- `adapter_misconfigured`

## Scholar Gateway Adapter

The Scholar Gateway adapter connects to
`https://connector.scholargateway.ai/mcp` and normalizes `semantic_search`
results into internal evidence records.

Supported authentication methods:

- OS keyring token lookup where available
- client-provided OAuth flow where supported
- `SCHOLAR_GATEWAY_ACCESS_TOKEN` for development and opt-in CI smoke tests

The engine must not store access tokens or refresh tokens in SQLite. SQLite may
store only auth status, expiry timestamp, scopes, account label, last error
type, and last successful connection timestamp.

The adapter must:

- support mocked responses in default tests
- support live tests only when `SCHOLAR_GATEWAY_LIVE=1`
- record search query, timestamp, and adapter status
- normalize citation metadata
- store DOI links where available
- record empty results as evidence gaps
- distinguish retrieval failure from evidence absence
- explicitly record coverage gaps when Scholar Gateway is weak for a topic,
  domain, timeframe, or source type

## Claim Extraction Engine

The claim extractor identifies factual, citation-worthy, and decision-relevant
claims from drafts, notes, retrieved summaries, and generated reports.

### ClaimRecord

```json
{
  "claim_id": "claim_0001",
  "run_id": "run_20260621_001",
  "text": "string",
  "claim_type": "empirical",
  "domain": "security",
  "importance": "high",
  "risk_level": "medium",
  "requires_citation": true,
  "created_from": "draft_section_002",
  "status": "needs_more_evidence",
  "evidence_ids": [],
  "contradiction_ids": [],
  "confidence": null,
  "human_review_required": false
}
```

### Claim Types

Supported claim types:

- `empirical`
- `definitional`
- `causal`
- `comparative`
- `quantitative`
- `historical`
- `legal`
- `medical`
- `security`
- `policy`
- `predictive`
- `normative`
- `methodological`
- `hypothesis`

Claim type determines evidence requirements. Causal claims require stronger
evidence than descriptive claims. Quantitative claims require exact source
support. Predictive claims should usually be marked as hypotheses. Normative
claims are not verified as factual claims. Medical, legal, financial, and
safety-sensitive claims require stricter gates.

## Claim Verification Engine

The verification engine determines whether each claim is supported,
contradicted, or unresolved by available evidence.

Allowed statuses:

- `supported`
- `partially_supported`
- `contradicted`
- `unsupported`
- `needs_more_evidence`
- `hypothesis`
- `not_verifiable`
- `out_of_scope`

### VerificationRecord

```json
{
  "verification_id": "ver_0001",
  "claim_id": "claim_0001",
  "method": "llm_judge_with_evidence",
  "evidence_ids": ["ev_0001", "ev_0002"],
  "support_status": "partially_supported",
  "support_rationale": "string",
  "missing_evidence": "string|null",
  "contradictory_evidence_ids": [],
  "confidence": 0.74,
  "requires_human_review": false,
  "verified_at": "ISO-8601"
}
```

### Blocking Rules

Final report mode blocks:

- unsupported factual claims
- quantitative claims without exact evidence
- claims with contradicted evidence presented as fact
- high-stakes claims without domain-required caveats
- claims based only on failed retrieval
- claims with citation mismatch
- claims whose cited source does not support the specific sentence

Draft mode may retain unsupported claims only when visibly labeled:

```text
Hypothesis, not yet supported by retrieved evidence.
```

## Evidence Ledger

The evidence ledger stores all evidence records used, rejected, ignored, or
marked risky.

It tracks:

- adapter source
- retrieval query
- rank and retrieval score
- citation metadata
- snippet hash
- source risk flags
- evidence quality score
- linked claims
- rejected reason where applicable

Evidence is not automatically trusted. It is registered, scored, and linked.

## Contradiction Graph

The contradiction graph records conflicts between claims or evidence.

```json
{
  "contradiction_id": "con_0001",
  "claim_a": "claim_0001",
  "claim_b": "claim_0007",
  "relationship": "contradicts",
  "evidence_for_a": ["ev_0001"],
  "evidence_for_b": ["ev_0009"],
  "resolution": "literature_split",
  "human_review_required": true
}
```

Contradiction states:

- `direct_contradiction`
- `scope_mismatch`
- `time_sensitive_difference`
- `jurisdiction_difference`
- `methodological_difference`
- `literature_split`
- `unresolved_conflict`

Reports must not flatten contested evidence into false certainty.

## Provenance Graph

The provenance graph records how the research output was produced. Events are
append-only during a run.

Minimum event types:

- `session_started`
- `run_created`
- `profile_selected`
- `mode_selected`
- `query_received`
- `research_plan_created`
- `retrieval_adapter_selected`
- `scholar_search_requested`
- `scholar_search_completed`
- `evidence_registered`
- `evidence_rejected`
- `claim_extracted`
- `claim_verified`
- `contradiction_detected`
- `quality_gate_run`
- `memory_candidate_created`
- `memory_saved`
- `review_item_created`
- `override_recorded`
- `report_exported`
- `run_pack_exported`
- `attestation_created`

### ProvenanceEvent

```json
{
  "event_id": "evt_0001",
  "run_id": "run_20260621_001",
  "timestamp": "ISO-8601",
  "event_type": "claim_verified",
  "actor": "engine",
  "input_refs": ["claim_0001", "ev_0001"],
  "output_refs": ["ver_0001"],
  "summary": "Claim verified as partially_supported.",
  "metadata": {}
}
```

Storage:

- SQLite tables for queryable state.
- JSONL export in each run pack for audit and replay.

## Run Pack Exporter

Every completed run exports a folder:

```text
runs/<session_id>/
  run.json
  provenance.jsonl
  evidence_ledger.json
  claims.json
  claim_evidence_links.json
  contradictions.json
  quality_gate.json
  report.md
  unresolved_gaps.md
  sources.bib
  methodology.md
  benchmark_trace.json
  attestation.json
```

The folder format is the primary artifact because it is inspectable,
scriptable, and diffable. ZIP export can be added later as a convenience
feature.

## Run Pack Attestation

Each run pack includes `attestation.json` to make tampering detectable.

```json
{
  "run_id": "run_20260621_001",
  "created_at": "ISO-8601",
  "engine_version": "10.0.0",
  "artifact_hashes": {
    "run.json": "sha256:...",
    "provenance.jsonl": "sha256:...",
    "evidence_ledger.json": "sha256:...",
    "claims.json": "sha256:...",
    "claim_evidence_links.json": "sha256:...",
    "quality_gate.json": "sha256:...",
    "report.md": "sha256:..."
  },
  "run_pack_hash": "sha256:...",
  "signature": null,
  "signature_algorithm": null
}
```

Future phases may support Ed25519 local signing, keyring-backed signing keys,
detached signature export, and offline verification commands.

## Report Quality Gate

The report quality gate prevents low-integrity reports from being exported as
final outputs.

Allowed results:

- `pass`
- `pass_with_warnings`
- `blocked`

Blocking failures:

- unsupported factual claims remain
- important claims lack citations
- contradicted claims are presented as fact
- evidence collection failed but the report implies success
- provenance is incomplete
- high-stakes caveats are missing
- citation links do not support claim text
- retrieval coverage is too weak for the selected mode
- source risk flags are unresolved
- manual override lacks rationale

Advisory warnings:

- source diversity is low
- synthesis is shallow
- citations are sparse but acceptable for the selected mode
- unresolved gaps are present but clearly disclosed
- latency exceeded target
- benchmark trace is incomplete but non-critical

## Verified Research Memory

Memory allows reuse of verified research knowledge only when the provenance is
safe enough to carry forward.

Allowed memory classes:

- `verified_claim`
- `method_note`
- `source_reliability`
- `domain_caveat`
- `contradiction_warning`
- `search_strategy`

### MemoryRecord

```json
{
  "memory_id": "mem_0001",
  "memory_type": "verified_claim",
  "claim_id": "claim_0001",
  "evidence_ids": ["ev_0001"],
  "profile": "security",
  "run_id": "run_20260621_001",
  "created_at": "ISO-8601",
  "expires_at": "ISO-8601",
  "confidence": 0.84,
  "decay_policy": "fast",
  "revalidation_required": true
}
```

Rules:

- unsupported claims must not be saved as facts
- contradicted claims may be saved only as warnings
- memory must include evidence IDs and run ID
- fast-moving domains require expiry
- memory reuse must be visible in provenance
- expired memory or high-stakes use requires revalidation

## Human Review Workflow

The engine escalates meaningful research judgment to a human instead of hiding
uncertainty in prose.

Create review items when:

- high-stakes mode is selected
- evidence is contradictory
- a useful claim is unsupported but may be kept as a hypothesis
- Scholar Gateway coverage is weak
- final report is blocked but could pass with caveats
- user goal conflicts with evidence standards
- citation support is ambiguous
- source risk flags indicate prompt injection or tool poisoning
- manual evidence import is used for a high-impact claim

### ReviewRecord

```json
{
  "review_id": "rev_0001",
  "run_id": "run_20260621_001",
  "item_type": "unsupported_high_value_claim",
  "claim_id": "claim_0008",
  "evidence_ids": [],
  "recommended_action": "mark_as_hypothesis",
  "decision": null,
  "reviewer": null,
  "rationale": null,
  "created_at": "ISO-8601"
}
```

All overrides must be recorded in provenance.

## Domain Profiles

Profiles live in editable YAML or JSON files.

Initial profiles:

- `general`
- `medicine`
- `law`
- `business`
- `engineering`
- `science`
- `humanities`
- `policy`
- `security`
- `ai_safety`

Each profile defines preferred source types, weak or disallowed source types,
minimum evidence count, recency requirements, citation style, claim strictness,
high-risk triggers, required caveats, and scoring weights.

Example:

```yaml
name: security
preferred_source_types:
  - peer_reviewed_article
  - conference_paper
  - standards_document
  - vendor_advisory
weak_source_types:
  - blog_post
  - social_media
minimum_evidence_count: 2
recency_requirement: fast
citation_style: harvard_au
claim_strictness: high
high_risk_triggers:
  - exploitability
  - vulnerability_disclosure
  - malware
  - credential_theft
required_caveats:
  - "Security findings may change quickly as patches and disclosures evolve."
scoring_weights:
  source_quality: 0.25
  citation_support: 0.30
  recency: 0.15
  contradiction_handling: 0.20
  provenance_completeness: 0.10
```

The engine chooses a profile automatically but allows user override.

## Research Modes

Supported modes:

- `quick`
- `standard`
- `deep`
- `scholarly`
- `audit`
- `high_stakes`

Default mode is `standard`.

| Mode | Purpose | Retrieval depth | Claim strictness | Output |
| --- | --- | ---: | ---: | --- |
| `quick` | fast orientation | low | medium | short answer |
| `standard` | normal research | medium | high | report |
| `deep` | detailed synthesis | high | high | long report |
| `scholarly` | academic-first research | high | very high | cited report |
| `audit` | verification and replay | variable | very high | ledgers and gate |
| `high_stakes` | medical/legal/financial/safety/security | high | maximum | cautious report and review |

The engine escalates to `high_stakes` for medical, legal, financial,
safety-critical, policy-sensitive, security-sensitive, vulnerability-impacting,
and public-health-impacting queries. Downgrading from `high_stakes` requires
explicit human confirmation and must be recorded in provenance.

## CLI Surface

Initial commands:

```bash
reasoning-engine mcp
reasoning-engine serve --transport http --host 127.0.0.1 --port 8765
reasoning-engine research "<query>" --mode standard --profile auto
reasoning-engine scholar search "<query>" --limit 10
reasoning-engine verify --run runs/<session_id>
reasoning-engine export --run runs/<session_id>
reasoning-engine attest --run runs/<session_id>
reasoning-engine benchmark --suite smoke
reasoning-engine benchmark diff --base runs/<old> --candidate runs/<new>
```

Rules:

- `mcp` starts the local STDIO MCP server.
- `serve --transport http` starts localhost HTTP MCP.
- HTTP binds to `127.0.0.1` by default.
- Binding to `0.0.0.0` requires `--unsafe-bind-public`.
- Public binding prints a warning and records the choice in provenance.
- HTTP mode supports an optional local bearer token.
- Secrets are read from environment, keyring, or interactive login flow, not
  stored in SQLite.

## MCP Tool Surface

### Research

- `start_research_run`
- `classify_research_mode`
- `select_research_profile`
- `create_research_plan`
- `run_research_pipeline`

### Scholar Gateway

- `scholar_search_tool`
- `register_scholar_evidence`
- `get_scholar_auth_status`

### Evidence

- `register_evidence`
- `get_evidence_ledger`
- `score_source_quality`
- `mark_evidence_gap`

### Claims

- `extract_claims_tool`
- `verify_claims_against_evidence`
- `get_claim_ledger`
- `get_claim_evidence_links`
- `get_contradiction_graph`

### Quality and Export

- `run_quality_gate`
- `export_run_pack`
- `create_attestation_manifest`
- `verify_run_pack_attestation`

### Memory

- `list_memory_candidates`
- `save_verified_memory`
- `revalidate_memory`

### Review

- `list_review_items`
- `record_review_decision`

### Benchmarks

- `run_research_benchmark`
- `diff_benchmark_results`

## Data Model

Use explicit migrations. Do not rely only on `CREATE TABLE IF NOT EXISTS`.

Initial tables:

- `research_profiles`
- `research_runs`
- `retrieval_adapters`
- `scholar_searches`
- `scholar_sources`
- `evidence_records`
- `evidence_gaps`
- `claims`
- `claim_evidence_links`
- `claim_verifications`
- `contradictions`
- `quality_gate_results`
- `provenance_events`
- `memory_items`
- `review_items`
- `benchmark_tasks`
- `benchmark_results`
- `attestation_manifests`

Storage split:

- SQLite stores queryable state.
- Run packs store exported audit artifacts.
- Keyring stores secrets.
- Environment variables may provide development-only tokens.

## Security and Privacy

Core rules:

- no general web crawling in the first implementation
- external content is untrusted
- tool metadata is untrusted
- evidence snippets are evidence, not instructions
- prompt-injection indicators are stored as risk metadata
- secrets are never stored in SQLite or run packs
- run packs may contain sensitive research context and stay local unless the
  user explicitly exports them
- local HTTP binds to `127.0.0.1` by default
- public binding requires explicit unsafe opt-in
- tool outputs are sanitized before entering traces or reports
- evidence registration does not execute source-provided instructions
- MCP tools prefer allow lists over broad tool exposure

Primary risks:

- prompt injection through retrieved content
- tool poisoning through MCP metadata
- OAuth token leakage
- accidental public HTTP exposure
- fake evidence from failed retrieval
- stale verified memory
- report tampering after export
- citation mismatch
- unsupported claims presented as fact
- local artifact leakage

Required controls:

- content sanitization
- source risk flags
- evidence hash chain
- tool allow/deny configuration
- local bearer token for HTTP mode
- keyring-based secret storage where possible
- provenance event logging
- attestation manifest
- quality gate blockers
- explicit human review for overrides

## Benchmark and Regression Harness

Benchmarks measure whether changes improve research quality, not just whether
code passes.

Initial suites:

- `smoke`
- `standard`
- `adversarial`
- `high_stakes`
- `regression`

Benchmark tasks should include normal research, contradictory evidence, source
gaps, unsupported-claim traps, citation mismatches, stale evidence,
prompt-injection sources, tool-poisoning metadata, high-stakes caveats, and
retrieval failures.

Required metrics:

- source relevance
- citation metadata completeness
- claim support coverage
- citation precision
- citation recall
- contradiction handling
- unresolved gap quality
- report completeness
- provenance completeness
- source risk handling
- quality gate accuracy
- latency
- cost
- regression delta

### Regression Diff

The benchmark system compares runs across versions.

```json
{
  "base_version": "9.4.0",
  "candidate_version": "10.0.0",
  "claim_support_coverage_delta": "+0.08",
  "unsupported_claim_rate_delta": "-0.04",
  "citation_precision_delta": "+0.06",
  "provenance_completeness_delta": "+0.12",
  "latency_delta_ms": "+480",
  "regression_detected": false
}
```

## Report Output Contract

Final reports include:

- answer summary
- scope and assumptions
- methodology
- key findings
- evidence-backed claims
- contradictory evidence if present
- unresolved gaps
- caveats
- source list
- run ID
- quality gate status

Final reports must not imply retrieval succeeded if retrieval failed. They must
not present hypotheses as verified facts or hide important evidence gaps.

## Error Handling

The engine distinguishes:

- no evidence found
- retrieval failed
- authentication failed
- retrieval succeeded but evidence contradicted the claim
- retrieval succeeded but evidence was weak
- retrieval succeeded but source risk was detected

Error-to-report rules:

- `auth_required`: block scholarly retrieval and return setup instructions.
- `rate_limited`: retry with safe backoff or mark deferred.
- `unavailable`: mark evidence collection deferred.
- `empty_result`: record evidence gap.
- `malformed_response`: record adapter error and do not register evidence.
- `licence_restricted`: record metadata only if permitted.
- `adapter_misconfigured`: block retrieval and provide fix guidance.

## Testing Strategy

Unit tests:

- profile loading and validation
- mode selection
- claim type classification
- Scholar Gateway response normalization
- evidence scoring
- claim status transitions
- contradiction graph construction
- quality gate blockers
- run pack export
- attestation hashing
- memory expiry and decay
- error classification

Mocked integration tests:

- Scholar Gateway success
- auth failure
- rate limit
- empty result
- malformed response
- evidence registration
- claim verification flow
- quality gate block
- run pack export
- attestation verification

E2E tests:

- local STDIO MCP tool flow
- localhost HTTP MCP tool flow
- CLI research run
- CLI verify run
- CLI benchmark smoke suite
- unsupported claim blocked from final report
- retrieval failure correctly reported
- high-stakes query escalates mode
- tampered run pack fails attestation verification

Optional live tests:

```bash
SCHOLAR_GATEWAY_LIVE=1
```

Live tests never run in default CI.

## Implementation Phases

### Phase 1: Foundation

- CLI entrypoint
- transport-neutral service layer
- config loader
- migration scaffold
- profile loader
- mode selector
- provenance writer

### Phase 2: Scholar Gateway Adapter

- adapter interface
- Scholar Gateway adapter
- auth metadata model
- mock response fixtures
- `scholar_search_tool`
- typed retrieval errors

### Phase 3: Evidence and Claims

- evidence ledger
- claim extractor interface
- claim type taxonomy
- claim verification statuses
- source quality scoring
- claim evidence linking
- unsupported claim blockers

### Phase 4: Quality Gate and Run Packs

- quality gate
- report status outputs
- unresolved gaps file
- run pack exporter
- sources.bib export
- methodology.md export

### Phase 5: Attestation

- artifact hash manifest
- run pack root hash
- verify command
- tamper-detection tests
- optional signing interface stub

### Phase 6: Memory and Review

- verified memory classes
- expiry and decay policy
- memory deduplication
- human review item ledger
- override recording
- high-stakes review flow

### Phase 7: Benchmarks

- benchmark task schema
- smoke benchmark suite
- adversarial benchmark fixtures
- benchmark scoring
- regression diff command
- benchmark report export

## Non-Goals for First Implementation

- general web crawling
- hosted multi-tenant cloud service
- browser dashboard
- full remote MCP connector publishing
- full external benchmark integration
- automated legal, medical, or financial advice
- multi-user tenant isolation
- training on retrieved content
- storing full publisher content beyond permitted snippets and metadata

## Open Risks

### Scholar Gateway OAuth Complexity

Local OAuth flows may require careful handling depending on MCP client support
and platform behavior.

### Source Coverage Limits

Scholar Gateway is strong for Wiley scholarly content but not sufficient for
every domain, especially fast-moving AI and cybersecurity research.

### Claim Verification Ambiguity

LLM-based claim verification can be inconsistent. The system needs clear claim
types, evidence requirements, and regression benchmarks.

### Citation Faithfulness

A citation may be relevant without being the actual evidence used by the model.
The engine must test citation support at the claim level.

### High-Stakes Output Risk

Medical, legal, financial, and security-sensitive research requires stricter
caveats, review, and refusal behavior.

### Memory Staleness

Verified memory can become stale. Expiry, decay, and revalidation are mandatory.

### MCP Tool Poisoning

MCP tool metadata and external source text may carry malicious instructions.
Metadata validation and source-risk logging are required.

## Acceptance Criteria

The first implementation is successful when:

- Codex can use the local STDIO MCP server.
- Claude Code can use the local STDIO MCP server.
- A user can start localhost HTTP MCP on `127.0.0.1`.
- Scholar Gateway search can be called directly or mocked in tests.
- Default tests require no live Scholar Gateway credentials.
- Every important factual claim can be represented in the claim ledger.
- Unsupported factual claims are blocked from final report mode.
- Evidence gaps are explicit and exported.
- Contradictions can be represented in the contradiction graph.
- Each run exports a complete run pack.
- Each run pack includes an attestation manifest.
- Tampered run packs fail verification.
- Smoke benchmarks run locally.
- Regression diff can compare two benchmark runs.
- Tokens are not stored in SQLite.
- Public HTTP binding requires explicit unsafe opt-in.

## Professional Positioning

Recommended category:

> Verifiable Research Engine

Recommended one-line description:

> A local-first MCP research engine that produces claim-level,
> evidence-linked, provenance-recorded, and tamper-evident research run packs
> for Codex, Claude Code, and compatible agent workflows.

Recommended technical claim:

> The system does not merely cite sources. It records which claims were made,
> which evidence supports them, which gaps remain unresolved, how the report was
> produced, and whether the exported run pack has been modified.

Avoid claiming:

- hallucination-free output
- fully automated truth
- universal scholarly coverage
- legal, medical, or financial authority
- Scholar Gateway coverage for all research domains
- cryptographic proof before attestation and verification are implemented

## References

- Anthropic. [Connect Claude Code to tools via MCP](https://code.claude.com/docs/en/mcp).
  Used for MCP connection patterns, HTTP/STDIO options, OAuth support, and
  prompt-injection warnings.
- Crossref. [REST API documentation](https://www.crossref.org/documentation/retrieve-metadata/rest-api/).
  Used for future metadata adapter planning.
- Model Context Protocol. [Transports](https://modelcontextprotocol.io/specification/2025-03-26/basic/transports).
  Used for STDIO and Streamable HTTP transport boundaries.
- Model Context Protocol. [Authorization](https://modelcontextprotocol.io/specification/2025-06-18/basic/authorization).
  Used for OAuth and HTTP authorization boundaries.
- OpenAI. [Codex configuration reference](https://developers.openai.com/codex/config-reference).
  Used for Codex MCP compatibility.
- Scholar Gateway / Wiley. [Getting connected and troubleshooting](https://docs.scholargateway.ai/).
  Used for endpoint, OAuth, supported operation, content coverage, and
  limitations.
- Semantic Scholar. [Academic Graph API](https://api.semanticscholar.org/api-docs/graph).
  Used for future scholarly metadata and citation adapter planning.
- arXiv. [API user manual](https://info.arxiv.org/help/api/user-manual.html).
  Used for future preprint adapter planning.
- DEER. [A benchmark for evaluating deep research agents on expert report generation](https://arxiv.org/html/2512.17776v3).
  Used for claim-level verification and expert-report evaluation concepts.
- Nature. [Synthesizing scientific literature with retrieval-augmented language models](https://www.nature.com/articles/s41586-025-10072-4).
  Used for citation recall, precision, coverage, coherence, and factuality
  evaluation concepts.
- arXiv. [Retrieval Augmented Generation Evaluation in the Era of Large Language Models](https://arxiv.org/html/2504.14891v1).
  Used for retrieval, generation, safety, and system-level evaluation
  separation.
- arXiv. [Model Context Protocol threat modeling and prompt-injection analysis](https://arxiv.org/html/2603.22489v1).
  Used for prompt-injection, tool-poisoning, authentication, and trace-auditing
  threat modeling.
