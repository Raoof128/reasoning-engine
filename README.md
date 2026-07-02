# Reasoning Engine

An MCP server that brings difficulty-adaptive, multi-path reasoning to Claude Code. It implements the Actor-Critic-Planner-Reflexion (ACPR) pipeline for deep research synthesis.

## What It Does

You give it a research question. It decides how hard the question is, allocates compute accordingly, explores multiple reasoning paths in parallel, scores each path, self-corrects weak paths, and synthesizes the best results into a coherent report.

```
"What is a Process Reward Model?"
  -> difficulty 0.21 -> single pass -> done in seconds

"How do PRMs interact with MCTS for test-time compute scaling?"
  -> difficulty 0.71 -> forest strategy -> 8 branches, 3 reflexion rounds
```

## Architecture

Two components work together:

```
Claude Code (LLM-powered orchestrator)
  |  runs the ACPR loop sequentially in one context — no subagents
  |  calls MCP tools for algorithmic decisions and evidence retrieval
  v
Reasoning Engine MCP Server (deterministic Python backend)
  - Difficulty estimation
  - DORA budget allocation (explore vs exploit)
  - UCB branch selection
  - Dual-signal PRM scoring (Promise + Progress)
  - Research angle planning and evidence-gap checks
  - Real academic search: OpenAlex, Crossref, arXiv, Semantic Scholar,
    Europe PMC, DOAJ, DBLP (free, no auth), deduplicated and ranked
  - Verifiable pipeline: claim extraction, evidence verification,
    quality gate, attested run-pack export
  - Tree state management (SQLite)
  - Episodic memory for cross-session learning
  - Content sanitization (prompt injection protection)
```

No API key required for the reasoning loop or academic search. Runs on your Claude Code Max subscription.

## How It Works

### The ACPR Pipeline

| Phase | What Happens |
|-------|-------------|
| **Initialize** | Estimate difficulty, allocate budget, recall past learnings |
| **Generate** | Investigate each research angle sequentially, gathering evidence via real academic search |
| **Evaluate** | Self-critique each path on Promise (will it succeed?) and Progress (is it advancing?) |
| **Plan** | DORA computes score variance (kappa) and decides: explore broadly or exploit the best path |
| **Reflect** | Low-scoring paths get textual critique injected back for self-correction |
| **Loop** | Repeat until budget exhausted or high-confidence result found |
| **Synthesize** | Top paths merged into a coherent research report |
| **Verify** | Machine-check the drafted claims against real evidence; blocks on unsupported claims |

The entire loop runs sequentially in one Claude Code conversation — no subagent spawning required.

### Difficulty-Adaptive Scaling

| Difficulty | Strategy | Branches | Reflexion |
|-----------|----------|----------|-----------|
| 0.0 - 0.3 | Single pass | 1 | None |
| 0.3 - 0.5 | Best-of-N | 3 | 1 round |
| 0.5 - 0.7 | Beam search | 5 | 2 rounds |
| 0.7 - 1.0 | Forest | 8 | 3 rounds |

## Installation

### 1. Clone and install

```bash
git clone https://github.com/Raoof128/reasoning-engine.git
cd reasoning-engine
python3 -m venv .venv
source .venv/bin/activate
pip install -e ".[dev]"
```

### 2. Run tests

```bash
pytest -v
```

### 3. Configure Claude Code

Add to your project's `.mcp.json`:

```json
{
  "mcpServers": {
    "reasoning-engine": {
      "command": "/path/to/reasoning-engine/.venv/bin/reasoning-engine",
      "args": ["mcp"]
    }
  }
}
```

Or register it for every project at once:

```bash
claude mcp add reasoning-engine -s user -- /path/to/reasoning-engine/.venv/bin/reasoning-engine mcp
```

### 4. Install the skill (optional)

Copy `skill/deep-research.md` to `~/.claude/skills/deep-research.md` for the `/deep-research` slash command.

## Required Skills and MCP Servers

This agent works with the following Claude Code components:

### Required MCP Servers

| MCP Server | Purpose | How to Get |
|-----------|---------|-----------|
| **reasoning-engine** | Core reasoning backend, plus built-in academic search (this repo) | Install from this repo |

No external web-crawling MCP server is required — evidence retrieval uses the reasoning engine's own `scholar_search_tool`, backed by OpenAlex, Crossref, arXiv, Semantic Scholar, Europe PMC, DOAJ, and DBLP.

### Required Skills (for full pipeline)

| Skill | Purpose | Pipeline Phase | Source |
|-------|---------|---------------|--------|
| **deep-research** | Orchestrates the ACPR reasoning loop | Phases 1-8 | Included in this repo |
| **[stop-slop](https://github.com/hardikpandya/stop-slop)** | Removes AI writing patterns from synthesis | Phase 9 | by [Hardik Pandya](https://github.com/hardikpandya/stop-slop) |
| **[docx](https://github.com/anthropics/skills/tree/main/skills/docx)** | Generates publication-quality Word documents | Phase 10 | by [Anthropic](https://github.com/anthropics/skills/tree/main/skills/docx) |

### Optional Skills

| Skill | Purpose | Source |
|-------|---------|--------|
| **[theme-factory](https://github.com/anthropics/skills/tree/main/skills/theme-factory)** | Apply visual themes to the output document | by [Anthropic](https://github.com/anthropics/skills/tree/main/skills/theme-factory) |

Install the deep-research skill:

```bash
cp skill/deep-research.md ~/.claude/skills/
```

The stop-slop and docx skills are third-party — see their repos for installation.

## MCP Tools

| Tool | Purpose |
|------|---------|
| `init_research_session` | Create session, estimate difficulty, allocate budget |
| `register_branch` | Register a reasoning branch with trace and sources |
| `score_branch` | Record dual-signal score (Promise + Progress + critique) |
| `select_next_branches` | DORA allocation: explore vs exploit based on kappa |
| `check_termination` | Should we stop? (budget, confidence, convergence) |
| `consensus_candidates` | Top-K branches for final synthesis |
| `record_reflection_tool` | Store a Reflexion cycle's critique and revision |
| `recall_memory_tool` | Retrieve relevant learnings from past sessions |
| `save_to_memory` | Persist episodic memory for future recall |
| `sanitize_content` | Strip HTML, scripts, and prompt injection patterns |
| `get_session_state` | Full session state for debugging |
| `plan_research_angles_tool` | Create prioritized research angles and starter questions |
| `evidence_gap_questions_tool` | Generate verification questions for claims before synthesis |
| `start_research_run` | Create a verifiable research run (classifies mode + profile) |
| `classify_research_mode_tool` | Classify or escalate a query's research mode (e.g. `high_stakes`) |
| `scholar_search_tool` | Real academic search across 7 free sources, deduplicated and ranked |
| `get_scholar_auth_status` | Report optional live-token availability without exposing values |
| `run_research_pipeline_tool` | Full pipeline: search, extract claims, verify, quality gate, export run pack |
| `run_quality_gate_tool` | Evaluate persisted claims and verifications for a run |
| `export_run_pack_tool` | Run the pipeline and return the exported, attested run-pack path |

## Project Structure

```
reasoning-engine/
  src/reasoning_engine/
    server.py       # MCP server wiring all tools
    cli.py          # reasoning-engine CLI (mcp, serve, research, scholar)
    transport.py    # MCP transport factory and local HTTP safety checks
    db.py           # SQLite schema and connections
    difficulty.py   # Heuristic difficulty estimator
    dora.py         # DORA budget allocation + branch selection
    ucb.py          # UCB1 explore/exploit selection
    sessions.py     # Session and branch lifecycle management
    memory.py       # Episodic memory for Reflexion learnings
    research.py     # Research angle and evidence-gap planning
    sanitizer.py    # Content sanitization for web data
    validation.py   # Shared MCP tool input validation helpers
    verifiable/      # Claim extraction, verification, quality gate, run-pack export
      aggregator.py  # Fan-out academic search across all sources, dedup + rank
      sources/       # OpenAlex, Crossref, arXiv, Semantic Scholar, Europe PMC,
                      #   DOAJ, DBLP, and optional Scholar Gateway adapters
  tests/            # 143 tests, all passing
  skill/            # Claude Code skill file
```

## Background

This project implements ideas from three research documents on AI reasoning architectures:

- **Process Reward Models** score individual reasoning steps (not just final answers), enabling dense feedback for tree search.
- **DORA (Direction-Oriented Resource Allocation)** uses score variance to dynamically switch between exploring many paths and exploiting the best one.
- **Reflexion** injects textual critiques back into the prompt, enabling self-correction without weight updates.
- **UCB1 selection** balances trying promising branches against exploring undervisited ones.
- **ReAct / Self-RAG style evidence checks** keep retrieval and verification explicit before synthesis.

The key insight: a Claude Code skill can orchestrate this entire pipeline sequentially in a single conversation on a Max subscription, with a lightweight Python MCP server handling the deterministic math and real academic retrieval. No separate API key needed.

## Documentation

- [Architecture Overview](docs/architecture.md)
- [API Reference](docs/api-reference.md)
- [Usage Examples](docs/examples.md)
- [Contributing](CONTRIBUTING.md)
- [Security Policy](SECURITY.md)
- [Code of Conduct](CODE_OF_CONDUCT.md)

## Verifiable Research Engine MVP

Academic search is **live by default** — no API key needed. It fans out
concurrently across seven free sources (OpenAlex, Crossref, arXiv, Semantic
Scholar, Europe PMC, DOAJ, DBLP), deduplicates by DOI/title, and ranks the
results:

```bash
reasoning-engine scholar search "MCP prompt injection" --limit 3
```

Run the full verifiable research pipeline (search, claim extraction,
verification, quality gate, attested run-pack export):

```bash
reasoning-engine research "Retrieval augmented generation reduces hallucination" \
  --draft "Retrieval augmented generation reduces hallucination in large language models."
```

Useful environment variables:

| Variable | Effect |
|----------|--------|
| `REASONING_ENGINE_OFFLINE=1` | Force deterministic mock evidence (tests, CI, no network) |
| `REASONING_ENGINE_SOURCES=openalex,crossref` | Narrow which free sources are enabled |
| `REASONING_ENGINE_CONTACT_EMAIL` | Contact address sent to OpenAlex/Crossref's "polite pool" |
| `S2_API_KEY` | Optional Semantic Scholar key for higher rate limits |

The paid Scholar Gateway connector remains available as one more source when configured:

```bash
export SCHOLAR_GATEWAY_LIVE=1
export SCHOLAR_GATEWAY_ACCESS_TOKEN="<token>"
reasoning-engine scholar search "literature synthesis evaluation" --limit 5
```

Tokens are read from environment or local credential mechanisms and are never
stored in SQLite or run packs. Claim verification uses deterministic lexical
overlap as an MVP placeholder verifier, so it is suitable for pipeline testing
and audit workflow validation rather than final semantic claim verification.

## Local HTTP MCP

STDIO remains the default MCP workflow. To start a local Streamable HTTP MCP
server:

```bash
reasoning-engine serve --transport http --host 127.0.0.1 --port 8765
```

The MCP endpoint is available at:

```text
http://127.0.0.1:8765/mcp
```

Public binding is blocked unless explicitly acknowledged:

```bash
reasoning-engine serve --transport http --host 0.0.0.0 --unsafe-bind-public
```

For Notion AI Custom MCP testing through a Cloudflare HTTPS tunnel, use the
laptop launcher:

```bash
chmod +x ./run-notion-mcp-laptop.sh
./run-notion-mcp-laptop.sh
```

On macOS, you can also double-click `run-notion-mcp-laptop.command` from
Finder to start the same launcher in Terminal.

The launcher keeps the MCP server bound to `127.0.0.1`, creates a local bearer
token file at `~/.reasoning-engine/notion-http.env`, starts a temporary
Cloudflare Tunnel, and prints the Notion MCP URL. See
[Notion Laptop MCP Tunnel](docs/notion-laptop-mcp.md).

The project requires `mcp>=1.24.0,<2`, which is above the `1.23.0` safety floor
for default FastMCP DNS rebinding protection.

## License

MIT
