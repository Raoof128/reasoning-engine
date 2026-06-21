# Research Engine 10/10 Design

Date: 2026-06-21

## Purpose

Upgrade `reasoning-engine` from a hardened research-orchestration MCP into a
professional, auditable research system for Codex and Claude Code local workflows.
The system should support broad topics while using Scholar Gateway as the first
trusted scholarly evidence source. It must stay local-first: users run it on their
own device, store run artifacts locally, and connect agents through local MCP
transports.

## Approved Direction

Build a transport-neutral core with three interfaces:

1. **Local STDIO MCP**: default and first-class interface for Codex and Claude Code.
2. **Localhost HTTP MCP**: optional local server mode started by the user from the
   terminal, bound to `127.0.0.1` by default.
3. **Future cloud remote MCP**: out of scope for the first implementation, but the
   core should not prevent a later hosted connector.

No web crawling is in scope. Scholarly retrieval uses Scholar Gateway directly.

## Source Context

The design is based on current platform and research constraints:

- Codex supports configured MCP servers, including STDIO and streaming HTTP, via
  user or project `config.toml`.
- Claude supports local MCP workflows and remote MCP connectors; remote connectors
  require public reachability and OAuth.
- Scholar Gateway exposes an MCP endpoint at
  `https://connector.scholargateway.ai/mcp`, uses OAuth 2.1 via CONNECT SSO, and
  currently documents `semantic_search`.
- Scholar Gateway provides peer-reviewed Wiley content, citation metadata, DOI
  links, text-only content, and semantic-similarity ranking.
- Deep research evaluation literature points toward claim-level verification,
  citation support scoring, machine-readable provenance, and benchmarked quality
  scoring rather than citation presence alone.

## Architecture

```text
Core research engine
  domain profiles
  mode selector
  Scholar Gateway adapter
  evidence ledger
  claim verifier
  provenance graph
  report quality gate
  run pack exporter
  verified memory
  benchmark harness
        |
        +-- CLI commands
        +-- Local STDIO MCP tools
        +-- Localhost HTTP MCP mode
        +-- Future hosted remote MCP mode
```

The core modules should not depend on MCP transport objects. MCP tools and CLI
commands call the same service layer.

## Major Components

### 1. Scholar Gateway Adapter

Responsible for authenticated `semantic_search` calls to Scholar Gateway and
normalizing results.

Behavior:

- Connect to `https://connector.scholargateway.ai/mcp`.
- Support OAuth token lookup from OS keyring when available.
- Support `SCHOLAR_GATEWAY_ACCESS_TOKEN` for development and CI smoke tests.
- Never store access or refresh tokens in SQLite.
- Store only auth metadata in SQLite: status, expiry, scopes, account label, and
  last error.
- Return typed errors: `auth_required`, `rate_limited`, `unavailable`,
  `malformed_response`, and `empty_result`.

The first implementation may use mocked Scholar Gateway responses for tests and
make live tests opt-in with `SCHOLAR_GATEWAY_LIVE=1`.

### 2. Strict Claim Verification Engine

Responsible for claim extraction, evidence linking, and support status.

Statuses:

- `supported`
- `partially_supported`
- `contradicted`
- `unsupported`
- `needs_more_evidence`
- `hypothesis`

Rules:

- Unsupported factual claims are blocked from final synthesis in normal report
  mode.
- Draft mode may retain unsupported claims only if visibly marked.
- Human/LLM override can include weak claims only as `hypothesis` or with explicit
  caveat text.
- Every final factual claim must link to evidence or an explicit unresolved-gap
  record.

### 3. Machine-Readable Provenance Graph

Responsible for recording how outputs were produced.

Minimum event types:

- `session_started`
- `profile_selected`
- `mode_selected`
- `research_angle_created`
- `scholar_search_requested`
- `scholar_search_completed`
- `evidence_registered`
- `claim_extracted`
- `claim_verified`
- `quality_gate_run`
- `memory_candidate_created`
- `memory_saved`
- `human_escalation_requested`
- `override_recorded`
- `report_exported`

Storage:

- SQLite tables for queryable state.
- JSONL export in each run pack for audit and replay.

### 4. Benchmark and Quality Scoring Harness

Responsible for measuring whether changes improve research quality.

Start with an internal benchmark:

- 20 to 50 curated tasks across medicine, law, business, engineering, science,
  humanities, policy, and general knowledge.
- A small CI subset of 3 to 5 tasks.
- Full benchmark run manually or nightly.

Metrics:

- source relevance
- citation metadata completeness
- claim support coverage
- contradiction handling
- unsupported claim rate
- report completeness
- provenance completeness
- latency

External benchmark adapters can be added later.

### 5. Config-Driven Domain Profiles

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

Each profile defines:

- preferred source types
- weak or disallowed source types
- minimum evidence count
- recency requirements
- citation style
- claim strictness
- high-risk triggers
- required caveats
- scoring weights

The engine chooses a profile automatically but allows user override.

### 6. Report Quality Gate

Responsible for final acceptance before report export.

Blocking failures:

- unsupported factual claims remain
- important claims lack citations
- contradicted claims are presented as fact
- provenance is incomplete
- domain-required caveats are missing
- evidence collection failed but the report implies it succeeded

Advisory warnings:

- shallow synthesis
- weak structure
- low source diversity
- acceptable but sparse citations
- optional evidence gaps

Outputs:

- `pass`
- `pass_with_warnings`
- `blocked`

### 7. Reproducible Research Run Pack

Every completed run exports a folder:

```text
runs/<session_id>/
  run.json
  provenance.jsonl
  evidence_ledger.json
  claims.json
  claim_evidence_links.json
  quality_gate.json
  report.md
  unresolved_gaps.md
  sources.bib
  methodology.md
```

The folder format is the primary artifact because it is inspectable, scriptable,
and diffable. ZIP export can be a later convenience feature.

### 8. Verified Research Memory

Memory is automatic but gated.

Allowed memory classes:

- `verified_claim`
- `method_note`
- `source_reliability`
- `domain_caveat`
- `contradiction_warning`
- `search_strategy`

Rules:

- Save only verified claims as reusable facts.
- Save contradicted claims only as warnings.
- Attach evidence IDs, profile, run ID, timestamp, and expiry where relevant.
- Deduplicate similar memories.
- Do not save unsupported claims as facts.

### 9. Human Review and LLM Escalation Workflow

The LLM handles routine decisions. The engine escalates meaningful research
judgment calls.

Escalate when:

- high-stakes profile is selected
- a high-value claim is unsupported but useful as a hypothesis
- evidence is contradictory and report direction depends on interpretation
- Scholar Gateway coverage is weak
- final report is blocked but could pass with caveats
- user goal conflicts with evidence quality standards

Every override is recorded in provenance.

### 10. Automatic Research Mode Selector

Modes:

- `quick`
- `standard`
- `deep`
- `scholarly`
- `audit`
- `high_stakes`

Default is `standard`. The engine escalates to `high_stakes` for medical, legal,
financial, safety, policy, and security-sensitive queries. Human confirmation is
required for high-stakes mode or downgrade from high-stakes mode.

## CLI Surface

Initial commands:

```bash
reasoning-engine mcp
reasoning-engine serve --transport http --host 127.0.0.1 --port 8765
reasoning-engine research "<query>" --mode standard --profile auto
reasoning-engine scholar search "<query>" --limit 10
reasoning-engine verify --run runs/<session_id>
reasoning-engine export --run runs/<session_id>
reasoning-engine benchmark --suite smoke
```

Rules:

- `mcp` starts the local STDIO MCP server.
- `serve --transport http` starts local HTTP MCP on localhost only by default.
- Binding to `0.0.0.0` requires an explicit unsafe flag and warning.
- HTTP mode should support an optional local bearer token.

## MCP Tool Surface

Keep existing tools and add new ones in groups.

Scholar Gateway:

- `scholar_search_tool`
- `register_scholar_evidence`

Claims and evidence:

- `extract_claims_tool`
- `verify_claims_against_scholar_evidence`
- `get_evidence_ledger`
- `get_claim_ledger`
- `source_quality_score`

Profiles and modes:

- `list_research_profiles`
- `select_research_profile`
- `classify_research_mode`

Run packs and quality:

- `run_quality_gate`
- `export_run_pack`
- `get_provenance_events`

Benchmarks:

- `run_research_benchmark`

Human escalation:

- `list_review_items`
- `record_review_decision`

## Data Model Additions

New tables:

- `research_profiles`
- `research_runs`
- `scholar_searches`
- `scholar_sources`
- `claims`
- `claim_evidence_links`
- `quality_gate_results`
- `provenance_events`
- `memory_items`
- `review_items`
- `benchmark_tasks`
- `benchmark_results`

Schema changes should use explicit migrations rather than only
`CREATE TABLE IF NOT EXISTS`.

## Security and Privacy

- No crawling.
- Scholar Gateway is the only initial external retrieval source.
- Tokens are stored in keyring, not SQLite.
- Local HTTP binds to `127.0.0.1` by default.
- Local HTTP broad binding requires explicit unsafe opt-in.
- All external content is treated as untrusted.
- Evidence snippets are sanitized before entering branch traces or final reports.
- Prompt-injection indicators are recorded as source risk metadata.
- Run packs may contain sensitive research context and should stay local unless
  the user exports them.

## Error Handling

Scholar Gateway errors must not create fake evidence.

Required handling:

- `auth_required`: return setup instructions and block scholarly retrieval.
- `rate_limited`: retry with backoff only if safe, otherwise defer.
- `unavailable`: mark evidence collection deferred.
- `empty_result`: record a gap, not a failure.
- `malformed_response`: record error and do not register evidence.

Report generation must distinguish between no evidence found, retrieval failed,
and evidence contradicted the claim.

## Testing Strategy

Unit tests:

- profile loading and validation
- mode selection
- Scholar Gateway response normalization
- evidence scoring
- claim status transitions
- quality gate blockers
- run pack export

Mocked integration tests:

- Scholar Gateway success
- auth failure
- empty results
- rate limit
- malformed response

E2E tests:

- local MCP tool flow with mocked Scholar Gateway
- CLI research run with run pack export
- quality gate blocks unsupported claims
- benchmark smoke suite

Optional live tests:

- gated by `SCHOLAR_GATEWAY_LIVE=1`
- never run in default CI

## Implementation Phases

### Phase 1: Foundation

- CLI entrypoint
- transport-neutral service layer
- profile loader
- mode selector
- provenance event writer
- migration scaffold

### Phase 2: Scholar Gateway

- remote MCP adapter
- auth metadata model
- result normalization
- mocked integration tests
- `scholar_search_tool`

### Phase 3: Evidence and Claims

- evidence ledger
- claim extractor interface
- claim verification statuses
- source quality scoring
- strict blockers for unsupported factual claims

### Phase 4: Run Packs and Quality Gate

- run pack exporter
- quality gate
- report status outputs
- unresolved gaps file

### Phase 5: Memory and Review

- verified memory classes
- memory gating
- human review item ledger
- override recording

### Phase 6: Benchmarks

- internal benchmark task format
- smoke benchmark suite
- scoring output
- optional external benchmark adapters

## Non-Goals for First Implementation

- Full cloud-hosted remote MCP connector.
- General web crawling.
- Browser dashboard.
- ZIP-only artifact format.
- Full external benchmark integration.
- Multi-user tenant isolation.

## Open Risks

- Scholar Gateway OAuth from a local Python process may require an SDK pattern or
  browser-based OAuth flow that needs careful implementation.
- Scholar Gateway currently covers Wiley content; evidence gaps must be explicit
  for domains where coverage is weak.
- Strict verification may slow reports unless modes and thresholds are tuned.
- LLM-based claim extraction can miss implicit claims; benchmark tasks should
  measure this.

## Acceptance Criteria

The first implementation is successful when:

- Codex and Claude Code can use the local STDIO MCP server.
- A user can run local HTTP MCP on `127.0.0.1` from the terminal.
- Scholar Gateway search can be called directly or mocked in tests.
- Every important claim can be represented in a claim ledger.
- Unsupported factual claims are blocked from final report mode.
- Each run exports a machine-readable run pack.
- The smoke benchmark suite runs locally.
- Default tests do not require Scholar Gateway live credentials.

