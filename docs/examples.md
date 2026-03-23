# Usage Examples

Three scenarios showing the reasoning engine at different difficulty levels.

---

## Example 1: Simple Query (single_pass)

**Query:** "What is a Process Reward Model?"

### Step 1: Initialize

```
init_research_session(query="What is a Process Reward Model?")
```

Response:
```json
{
  "session_id": "abc-123",
  "difficulty": 0.21,
  "strategy": "single_pass",
  "budget": { "total_branches": 1, "max_depth": 3, "max_steps": 5 }
}
```

Low difficulty. One branch, no reflexion.

### Step 2: Register the single branch

```
register_branch(
  session_id="abc-123",
  trace='["Step 1: PRMs score individual reasoning steps", "Step 2: Dense per-step feedback"]',
  sources='[{"url": "https://arxiv.org/abs/2305.20050", "title": "Lets Verify Step by Step"}]'
)
```

### Step 3: Score it

```
score_branch(session_id="abc-123", branch_id="br-1", q_score=0.88, advantage=0.6,
  critique="Clear explanation, good source", confidence=0.85)
```

### Step 4: Check termination

```
check_termination(session_id="abc-123")
-> { "should_terminate": true, "reason": "High confidence result (q=0.88, conf=0.85)" }
```

Done in one pass.

---

## Example 2: Medium Query (beam_search)

**Query:** "Compare beam search and best-of-N sampling for mathematical reasoning"

### Step 1: Initialize

```
init_research_session(query="Compare beam search and best-of-N sampling for mathematical reasoning")
```

Response:
```json
{
  "session_id": "def-456",
  "difficulty": 0.55,
  "strategy": "beam_search",
  "budget": { "total_branches": 5, "max_depth": 8, "max_steps": 30 }
}
```

### Step 2: Spawn 5 Actor agents with different angles

- Agent 1: Theoretical comparison (search topology properties)
- Agent 2: Empirical benchmarks (MATH, GSM8K results)
- Agent 3: Compute cost analysis (parallel vs sequential)
- Agent 4: PRM integration differences
- Agent 5: Practical implementation patterns

Register each as a branch.

### Step 3: Score all 5 branches

Results: [0.72, 0.45, 0.68, 0.81, 0.55]

### Step 4: DORA selects

```
select_next_branches(session_id="def-456")
-> {
  "branches_to_continue": ["br-4", "br-1", "br-3"],
  "branches_to_reflect": ["br-5"],
  "branches_to_prune": ["br-2"],
  "allocation": "breadth",
  "kappa": 0.36
}
```

High kappa (0.36) means scores are spread. DORA explores broadly. Branch 2 (0.45) gets pruned. Branch 5 (0.55) goes to Reflexion.

### Step 5: Reflexion on branch 5

Critic said: "Missing concrete code examples. Claims about implementation ease are unverified."

Reflexion agent crawls implementation guides, adds code snippets, re-registers as child branch. Re-scored at 0.71.

### Step 6: Continue and converge

After 2 more loops, top branch reaches q=0.87, conf=0.82. Terminates. Synthesis merges top 3 paths.

---

## Example 3: Complex Query (forest with reflexion)

**Query:** "How do process reward models interact with Monte Carlo tree search in the context of test-time compute scaling, and what are the implications for building autonomous research agents?"

### Step 1: Initialize

```json
{
  "session_id": "ghi-789",
  "difficulty": 0.71,
  "strategy": "forest",
  "budget": { "total_branches": 8, "max_depth": 12, "max_steps": 50 }
}
```

Forest strategy. 8 branches, 3 reflexion rounds, 50 step budget.

### Step 2: Spawn 8 Actor agents

- PRM architecture and training
- MCTS algorithms and search strategies
- Test-time compute scaling theory
- AgentPRM dual-signal framework
- Forest-of-Thought implementation
- Reflexion and self-correction
- Practical agent architectures (LangGraph, Agent SDK)
- Recent 2025-2026 advances

### Step 3: Score all 8

Results: [0.82, 0.75, 0.69, 0.88, 0.71, 0.63, 0.45, 0.77]

### Step 4: DORA — first pass

```json
{
  "allocation": "breadth",
  "kappa": 0.43,
  "branches_to_continue": ["br-4", "br-1", "br-8", "br-2", "br-5"],
  "branches_to_reflect": ["br-3", "br-6"],
  "branches_to_prune": ["br-7"]
}
```

Agent 7 (practical architectures, 0.45) pruned. Agents 3 and 6 go to Reflexion. High kappa means broad exploration continues.

### Step 5: Reflexion rounds

Agent 3 critique: "Test-time compute theory is too abstract. Needs concrete allocation formulas."
After reflexion: score improves 0.69 -> 0.78.

Agent 6 critique: "Reflexion coverage is shallow. Missing episodic memory component."
After reflexion: score improves 0.63 -> 0.74.

### Step 6: Second generation — extend top branches

Spawn deeper Actor agents on the 5 continuing branches. Each goes one level deeper into their angle.

### Step 7: Re-evaluate, DORA switches to depth

After second scoring round, kappa drops to 0.08. DORA switches to depth mode. Focus shifts to top 2 branches (AgentPRM framework + PRM architecture).

### Step 8: Termination

Top branch reaches q=0.91, conf=0.88. Terminates after 3 loops.

### Step 9: Synthesize

Top 3 branches merged. The synthesis covers:
- How PRMs provide dense Q-value signals for MCTS node evaluation
- How DORA allocates test-time compute based on PRM score variance
- How Reflexion enables agents to self-correct without weight updates
- Implications for building research agents that improve across sessions

Total: 28 web sources, 3 reflexion cycles, 50 MCP tool calls.
