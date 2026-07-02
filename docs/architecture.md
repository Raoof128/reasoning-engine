# Architecture Overview

## System Diagram

```
+------------------------------------------------------------------+
|                        Claude Code (Max)                          |
|                                                                   |
|  /deep-research skill (runs sequentially, no subagents)          |
|  +------------------------------------------------------------+  |
|  |  Phase 1: INIT         -> recall memory, estimate difficulty|  |
|  |  Phase 2: GENERATE     -> investigate each angle in turn    |  |
|  |  Phase 3: EVALUATE     -> self-critique each branch         |  |
|  |  Phase 4: PLAN         -> call DORA allocation              |  |
|  |  Phase 5: REFLECT      -> revise flagged branches in turn   |  |
|  |  Phase 6: LOOP/TERM    -> check termination conditions      |  |
|  |  Phase 7: SYNTHESIZE   -> merge top paths into report       |  |
|  |  Phase 8: VERIFY       -> machine-check claims vs evidence  |  |
|  |  Phase 9: SLOP AUDIT   -> remove AI writing patterns        |  |
|  |  Phase 10: DOCX OUTPUT -> generate Word document            |  |
|  +-----------------------------+------------------------------+  |
|                                |                                  |
|                          MCP tool calls                           |
+--------------------------------+----------------------------------+
                                 |
                    +------------v-------------+
                    |    reasoning-engine       |
                    |    MCP Server             |
                    |                           |
                    |  - Difficulty est.        |
                    |  - DORA allocation        |
                    |  - UCB selection          |
                    |  - Score tracking         |
                    |  - Tree state (SQL)       |
                    |  - Episodic memory        |
                    |  - Sanitization           |
                    |  - Academic search        |----> OpenAlex, Crossref,
                    |    (aggregator, 7 sources)|      arXiv, Semantic Scholar,
                    |  - Verifiable pipeline    |      Europe PMC, DOAJ, DBLP
                    |    (claims, quality gate, |      (free, HTTPS, no auth)
                    |     attested run packs)   |
                    +-------------+-------------+
                                  |
                           +------v------+
                           |   SQLite    |
                           |             |
                           | sessions    |
                           | branches    |
                           | reflections |
                           | episodic_   |
                           |   memory    |
                           | sources     |
                           | research_   |
                           |   runs      |
                           | evidence_*  |
                           | claims,     |
                           |   verif.    |
                           | provenance  |
                           +-------------+
```

## Components

### Claude Code Skill (`/deep-research`)

The orchestration layer. A prompt-based skill that turns Claude Code into a reasoning engine by:

- Running research (Actor), evaluation (Critic), and self-correction (Reflexion) sequentially in the current conversation — no subagent spawning
- Calling MCP tools for all algorithmic decisions and evidence retrieval
- Managing the state machine across 10 phases

Runs entirely on the user's Max subscription. No separate API key.

### Reasoning Engine MCP Server

A Python FastMCP server handling all deterministic logic. Claude Code calls it via MCP tool invocations. The server never makes LLM calls itself.

**Modules:**

| Module | Responsibility |
|--------|---------------|
| `server.py` | MCP tool wiring, JSON parsing, logging |
| `cli.py` | `reasoning-engine` CLI: `mcp`, `serve`, `research`, `scholar search` |
| `transport.py` | MCP transport factory, local HTTP bind safety checks |
| `db.py` | SQLite schema, connection management |
| `difficulty.py` | Heuristic query difficulty estimation |
| `dora.py` | Budget allocation + explore/exploit switching |
| `ucb.py` | UCB1 branch selection for tree search |
| `sessions.py` | Session and branch lifecycle CRUD |
| `memory.py` | Episodic memory for cross-session learning |
| `research.py` | Research angle planning and evidence-gap questions |
| `sanitizer.py` | Prompt injection and HTML stripping |
| `validation.py` | Shared MCP tool input validation and bounds |

### Verifiable Research Service (`verifiable/`)

A self-contained subsystem for evidence-backed claim verification, used by `scholar_search_tool`, `run_research_pipeline_tool`, and the skill's VERIFY phase.

| Module | Responsibility |
|--------|---------------|
| `aggregator.py` | Fans out to enabled academic sources concurrently, dedups by DOI/title, ranks, tolerates partial failure |
| `sources/` | One adapter per provider — `openalex.py`, `crossref.py`, `arxiv.py`, `semantic_scholar.py`, `europepmc.py`, `doaj.py`, `dblp.py`, plus the optional `scholar_gateway.py` connector and a `mock.py` fixture for offline/CI runs |
| `claims.py` | Extracts factual claims from a draft and verifies them against retrieved evidence |
| `quality.py` | Quality gate: blocks on unsupported/contradicted claims |
| `runpack.py` | Exports a hashed, tamper-evident run pack (claims, verifications, sources.bib, provenance, attestation) |
| `store.py` | SQLite persistence for runs, evidence, claims, verifications, and the provenance ledger |
| `profiles.py` | Domain profiles and high-stakes mode escalation |

All seven free sources (OpenAlex, Crossref, arXiv, Semantic Scholar, Europe PMC, DOAJ, DBLP) are live by default over HTTPS with no API key. `REASONING_ENGINE_OFFLINE=1` swaps in deterministic mock evidence. arXiv responses are parsed with `defusedxml`, since Atom feeds are untrusted network input.

## Data Flow

```
User query
    |
    v
[Difficulty Estimator] -> score 0.0-1.0
    |
    v
[DORA Budget Allocator] -> strategy, branches, depth, steps
    |
    v
[Sequential investigation, one angle at a time] --scholar_search_tool--> academic sources
    |                                                    |
    |                                              [Sanitizer]
    |                                                    |
    v                                                    v
[Branch Registry] <--------------------------- trace + clean sources
    |
    v
[Self-Critique] -> q_score (Promise) + advantage (Progress) + critique
    |
    v
[Score Branch] -> stored in SQLite
    |
    v
[DORA Select] -> compute kappa (score variance)
    |
    +-- kappa > 0.15 --> BREADTH: keep all branches
    +-- kappa < 0.15 --> DEPTH: focus on top 1-2
    |
    v
[Reflexion] -> inject critique, gather targeted evidence, revise, re-score
    |
    v
[Termination Check]
    |
    +-- q > 0.85 AND conf > 0.80 --> STOP
    +-- budget exhausted ----------> STOP
    +-- else ----------------------> LOOP back to Generate
    |
    v
[Consensus] -> top-K branches
    |
    v
[Synthesis] -> merged report draft
    |
    v
[Verifiable Pipeline] -> extract claims, verify against evidence, quality gate
    |
    +-- blocked -----> gather more evidence or revise claim, retry
    +-- pass --------> attested run pack (sources.bib, provenance, attestation)
    |
    v
[Episodic Memory] -> save learnings for future sessions
```

## Database Schema

Thirteen tables in SQLite: five for the ACPR reasoning loop, eight for the verifiable research pipeline.

### ACPR reasoning tables

**sessions** — One row per research session. Stores query, difficulty score, strategy, budget allocation, and status.

**branches** — One row per reasoning path. Stores the full trace (JSON array of steps), dual-signal scores (q_score for Promise, advantage for Progress), textual critique, confidence, and visit count for UCB. Linked to parent branch for tree structure.

**reflections** — One row per Reflexion cycle. Stores original critique, revision summary, and score delta (before/after).

**episodic_memory** — Cross-session learning store. Stores key learnings and domain tags from completed sessions. Queried by keyword overlap on future sessions.

**sources** — Web sources linked to branches. Stores URL, title, sanitized excerpt, and relevance score.

### Verifiable research tables

**research_runs** — One row per verifiable research run. Stores query, profile, mode, and status.

**evidence_records** — Retrieved academic evidence, one row per item, with a content hash for tamper detection.

**evidence_gaps** — Recorded when a search returns no evidence or a retrieval error, one per failed/empty search.

**claims** — Factual claims extracted from a draft, with type, domain, importance, risk level, and citation requirement.

**claim_verifications** — Verification outcome per claim (support status, rationale, confidence, evidence links).

**quality_gate_results** — Pass/blocked/pass-with-warnings outcome for a run, with blocking failures and warnings.

**provenance_events** — Append-only ledger of pipeline events (run created, profile/mode selected, searches, gaps), ordered by insertion sequence (`rowid`) for a trustworthy audit trail.

**attestation_manifests** — Hash of every exported run-pack artifact plus a combined run-pack hash, for tamper-evident verification.

## Key Algorithms

### Difficulty Estimation

Heuristic scoring (0.0-1.0) based on four signals:

- **Length** (25%): longer queries suggest more complex topics
- **Technical keywords** (35%): domain-specific terms (PRM, MCTS, RLHF, etc.)
- **Structural complexity** (25%): multi-part questions, comparisons, conjunctions
- **Sub-questions** (15%): multiple question marks

### DORA (Direction-Oriented Resource Allocation)

Maps difficulty to strategy (single_pass / best_of_n / beam_search / forest) and budget. Within a session, computes kappa (score range across branches) at each planning step:

- **kappa > 0.15**: Scores are spread out, uncertain. Explore broadly (breadth-first).
- **kappa < 0.15**: Scores converge, one path leads. Exploit best (depth-first).

### UCB1 Selection

Balances exploitation (high Q-score) with exploration (low visit count):

```
UCB(node) = Q(node) + c * sqrt(ln(parent_visits) / node_visits)
```

Unvisited nodes get infinity, guaranteeing they are tried. Exploration constant c defaults to 1.4.

### Dual-Signal Scoring

Each branch receives two scores from Critic agents:

- **Promise (Q-value)**: Probability this path leads to a comprehensive synthesis. Forward-looking.
- **Progress (Advantage)**: How much this step advanced beyond what was already known. Backward-looking.

This mirrors the AgentPRM framework from the research literature, enabling the Planner to distinguish between paths that look promising but stall versus paths that actively advance understanding.

## Design Decisions

| Decision | Chosen | Rationale |
|----------|--------|-----------|
| No API key | Sequential execution in one Claude Code conversation | Max subscription covers all LLM compute; no subagent orchestration overhead |
| No API key for evidence | Free academic sources (OpenAlex, Crossref, arXiv, etc.) | Real citations without a paid connector or credentials |
| SQLite | Persistent state | Zero-config, single-file, fast for small datasets |
| Text-based traces | Full reasoning text stored per branch | Claude reconstructs context from text (no encrypted tokens) |
| Heuristic difficulty | Keyword/structure analysis | No LLM call needed, sub-millisecond |
| Score range for kappa | max - min of Q-scores | Simpler and more interpretable than statistical variance |
| Content sanitization | Regex-based stripping | Defense-in-depth against prompt injection from web crawls |
| Concurrent source fan-out | `ThreadPoolExecutor` over 7 adapters | I/O-bound network calls; deterministic since dedup/rank run after collection |
| Provenance ordering | SQLite `rowid` (insertion order) | `created_at` is only second-granular and ties broke on a hash, scrambling the ledger |
