# Architecture Overview

## System Diagram

```
+------------------------------------------------------------------+
|                        Claude Code (Max)                          |
|                                                                   |
|  /deep-research skill                                            |
|  +------------------------------------------------------------+  |
|  |  Phase 1: INIT         -> recall memory, estimate difficulty|  |
|  |  Phase 2: GENERATE     -> spawn N parallel Actor agents     |  |
|  |  Phase 3: EVALUATE     -> spawn N parallel Critic agents    |  |
|  |  Phase 4: PLAN         -> call DORA allocation              |  |
|  |  Phase 5: REFLECT      -> spawn Reflexion agents            |  |
|  |  Phase 6: LOOP/TERM    -> check termination conditions      |  |
|  |  Phase 7: SYNTHESIZE   -> merge top paths into report       |  |
|  |  Phase 8: SLOP AUDIT   -> remove AI writing patterns        |  |
|  |  Phase 9: DOCX OUTPUT  -> generate Word document            |  |
|  +-----------------------------+------------------------------+  |
|                                |                                  |
|         MCP tool calls         |    Crawl4AI MCP calls           |
+--------------------------------+---------+-----------------------+
                                 |         |
              +------------------v--+    +-v-----------------+
              |  reasoning-engine   |    |    Crawl4AI        |
              |  MCP Server         |    |    MCP Server      |
              |                     |    |                    |
              |  - Difficulty est.  |    |  - Web crawling    |
              |  - DORA allocation  |    |  - Markdown extract|
              |  - UCB selection    |    |  - Content Q&A     |
              |  - Score tracking   |    +--------------------+
              |  - Tree state (SQL) |
              |  - Episodic memory  |
              |  - Sanitization     |
              +----------+----------+
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
                  +-------------+
```

## Components

### Claude Code Skill (`/deep-research`)

The orchestration layer. A prompt-based skill that turns Claude Code into a reasoning engine by:

- Spawning parallel agents for research (Actor), evaluation (Critic), and self-correction (Reflexion)
- Calling MCP tools for all algorithmic decisions
- Managing the state machine across 9 phases

Runs entirely on the user's Max subscription. No separate API key.

### Reasoning Engine MCP Server

A Python FastMCP server handling all deterministic logic. Claude Code calls it via MCP tool invocations. The server never makes LLM calls itself.

**Modules:**

| Module | Responsibility |
|--------|---------------|
| `server.py` | FastMCP wiring, JSON parsing, logging |
| `db.py` | SQLite schema, connection management |
| `difficulty.py` | Heuristic query difficulty estimation |
| `dora.py` | Budget allocation + explore/exploit switching |
| `ucb.py` | UCB1 branch selection for tree search |
| `sessions.py` | Session and branch lifecycle CRUD |
| `memory.py` | Episodic memory for cross-session learning |
| `sanitizer.py` | Prompt injection and HTML stripping |

### Crawl4AI MCP Server

A built-in Claude Code MCP server for web research. Actor and Reflexion agents use it to crawl URLs, extract markdown, and ask questions about page content. All crawled content passes through `sanitize_content` before entering the reasoning pipeline.

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
[Actor Agents] --crawl4ai--> web sources
    |                          |
    |                    [Sanitizer]
    |                          |
    v                          v
[Branch Registry] <--- trace + clean sources
    |
    v
[Critic Agents] -> q_score (Promise) + advantage (Progress) + critique
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
[Reflexion] -> inject critique, revise, re-score
    |
    v
[Termination Check]
    |
    +-- q > 0.85 AND conf > 0.80 --> STOP
    +-- budget exhausted ----------> STOP
    +-- else ----------------------> LOOP back to Actors
    |
    v
[Consensus] -> top-K branches
    |
    v
[Synthesis Agent] -> merged report
    |
    v
[Episodic Memory] -> save learnings for future sessions
```

## Database Schema

Five tables in SQLite:

**sessions** — One row per research session. Stores query, difficulty score, strategy, budget allocation, and status.

**branches** — One row per reasoning path. Stores the full trace (JSON array of steps), dual-signal scores (q_score for Promise, advantage for Progress), textual critique, confidence, and visit count for UCB. Linked to parent branch for tree structure.

**reflections** — One row per Reflexion cycle. Stores original critique, revision summary, and score delta (before/after).

**episodic_memory** — Cross-session learning store. Stores key learnings and domain tags from completed sessions. Queried by keyword overlap on future sessions.

**sources** — Web sources linked to branches. Stores URL, title, sanitized excerpt, and relevance score.

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
| No API key | Claude Code agent spawning | Max subscription covers all LLM compute |
| SQLite | Persistent state | Zero-config, single-file, fast for small datasets |
| Text-based traces | Full reasoning text stored per branch | Claude reconstructs context from text (no encrypted tokens) |
| Heuristic difficulty | Keyword/structure analysis | No LLM call needed, sub-millisecond |
| Score range for kappa | max - min of Q-scores | Simpler and more interpretable than statistical variance |
| Content sanitization | Regex-based stripping | Defense-in-depth against prompt injection from web crawls |
