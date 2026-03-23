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
  |  spawns parallel agents for generation, critique, reflexion
  |  calls MCP tools for algorithmic decisions
  v
Reasoning Engine MCP Server (deterministic Python backend)
  - Difficulty estimation
  - DORA budget allocation (explore vs exploit)
  - UCB branch selection
  - Dual-signal PRM scoring (Promise + Progress)
  - Tree state management (SQLite)
  - Episodic memory for cross-session learning
  - Content sanitization (prompt injection protection)
```

No API key required. Runs on your Claude Code Max subscription.

## How It Works

### The ACPR Pipeline

| Phase | What Happens |
|-------|-------------|
| **Initialize** | Estimate difficulty, allocate budget, recall past learnings |
| **Generate** | Spawn parallel Actor agents, each exploring a different research angle |
| **Evaluate** | Critic agents score each path on Promise (will it succeed?) and Progress (is it advancing?) |
| **Plan** | DORA computes score variance (kappa) and decides: explore broadly or exploit the best path |
| **Reflect** | Low-scoring paths get textual critique injected back for self-correction |
| **Loop** | Repeat until budget exhausted or high-confidence result found |
| **Synthesize** | Top paths merged into a coherent research report |

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
      "command": "/path/to/reasoning-engine/.venv/bin/fastmcp",
      "args": ["run", "/path/to/reasoning-engine/src/reasoning_engine/server.py"],
      "env": {
        "REASONING_ENGINE_DB": "/path/to/reasoning-engine/reasoning.db"
      }
    }
  }
}
```

### 4. Install the skill (optional)

Copy `skill/deep-research.md` to `~/.claude/skills/deep-research.md` for the `/deep-research` slash command.

## Required Skills and MCP Servers

This agent works with the following Claude Code components:

### Required MCP Servers

| MCP Server | Purpose | How to Get |
|-----------|---------|-----------|
| **reasoning-engine** | Core reasoning backend (this repo) | Install from this repo |
| **crawl4ai** | Web crawling for live research | Built-in Claude Code MCP |

### Required Skills (for full pipeline)

| Skill | Purpose | Pipeline Phase | Source |
|-------|---------|---------------|--------|
| **deep-research** | Orchestrates the ACPR reasoning loop | Phases 1-7 | Included in this repo |
| **[stop-slop](https://github.com/hardikpandya/stop-slop)** | Removes AI writing patterns from synthesis | Phase 8 | by [Hardik Pandya](https://github.com/hardikpandya/stop-slop) |
| **[docx](https://github.com/anthropics/skills/tree/main/skills/docx)** | Generates publication-quality Word documents | Phase 9 | by [Anthropic](https://github.com/anthropics/skills/tree/main/skills/docx) |

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

## Project Structure

```
reasoning-engine/
  src/reasoning_engine/
    server.py       # FastMCP server wiring all tools
    db.py           # SQLite schema and connections
    difficulty.py   # Heuristic difficulty estimator
    dora.py         # DORA budget allocation + branch selection
    ucb.py          # UCB1 explore/exploit selection
    sessions.py     # Session and branch lifecycle management
    memory.py       # Episodic memory for Reflexion learnings
    sanitizer.py    # Content sanitization for web data
  tests/            # 38 tests, all passing
  skill/            # Claude Code skill file
```

## Background

This project implements ideas from three research documents on AI reasoning architectures:

- **Process Reward Models** score individual reasoning steps (not just final answers), enabling dense feedback for tree search.
- **DORA (Direction-Oriented Resource Allocation)** uses score variance to dynamically switch between exploring many paths and exploiting the best one.
- **Reflexion** injects textual critiques back into the prompt, enabling self-correction without weight updates.
- **UCB1 selection** balances trying promising branches against exploring undervisited ones.

The key insight: a Claude Code skill can orchestrate this entire pipeline using parallel agent spawning on a Max subscription, with a lightweight Python MCP server handling the deterministic math. No separate API key needed.

## Documentation

- [Architecture Overview](docs/architecture.md)
- [API Reference](docs/api-reference.md)
- [Usage Examples](docs/examples.md)
- [Contributing](CONTRIBUTING.md)
- [Security Policy](SECURITY.md)
- [Code of Conduct](CODE_OF_CONDUCT.md)

## License

MIT
