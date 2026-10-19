# Reasoning Engine

An MCP server that brings difficulty-adaptive, multi-path reasoning to Claude Code. It implements the Actor-Critic-Planner-Reflexion (ACPR) pipeline for deep research synthesis, and verifies the claims it produces against real academic evidence.

Version 0.2.0 is a single static Rust binary. The Python package is deprecated (see [Migrating from Python](#migrating-from-python)).

## What It Does

You give it a research question. It decides how hard the question is, allocates compute accordingly, explores multiple reasoning paths, scores each path, self-corrects weak paths, synthesises the best results, and then checks every claim against retrieved evidence before it reaches you.

```
"What is a Process Reward Model?"
  -> difficulty 0.21 -> single pass -> done in seconds

"How do PRMs interact with MCTS for test-time compute scaling?"
  -> difficulty 0.71 -> forest strategy -> 8 branches, 3 reflexion rounds
```

## Architecture

```
Claude Code (LLM-powered orchestrator)
  |  runs the ACPR loop sequentially in one context — no subagents
  |  calls MCP tools for algorithmic decisions and evidence retrieval
  v
reasoning-engine (Rust, stdio MCP by default)
  - Difficulty estimation, DORA budget allocation, UCB1 branch selection
  - Dual-signal PRM scoring (Promise + Progress), Reflexion memory
  - Real academic search: OpenAlex, Crossref, arXiv, Semantic Scholar,
    Europe PMC, DOAJ, DBLP (free, no auth), fetched concurrently,
    deduplicated and ranked
  - Verifiable pipeline: claim extraction, lexical / hybrid / NLI
    verification, quality gate, attested run-pack export
  - SQLite state (same schema as the Python version), content sanitisation
  - Optional: local Streamable HTTP server and read-only audit UI
```

No API key is needed for the reasoning loop or academic search.

| | Python 0.1 | Rust 0.2 |
|---|---:|---:|
| Start-up (`--help`) | 617 ms | 3.1 ms |
| Offline research pipeline | 683 ms | 12 ms |

Measured with `scripts/bench-startup.sh` on one Linux machine, so read them as orders of magnitude rather than promises.

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

### Difficulty-Adaptive Scaling

| Difficulty | Strategy | Branches | Reflexion |
|-----------|----------|----------|-----------|
| 0.0 - 0.3 | Single pass | 1 | None |
| 0.3 - 0.5 | Best-of-N | 3 | 1 round |
| 0.5 - 0.7 | Beam search | 5 | 2 rounds |
| 0.7 - 1.0 | Forest | 8 | 3 rounds |

## Installation

Pick one.

### Release binary

Download the archive for your platform from [Releases](https://github.com/Raoof128/reasoning-engine/releases): `linux-x86_64` and `linux-aarch64` (static, any distribution), `macos-universal`, or `windows-x86_64`. Then verify it and put `reasoning-engine` on your `PATH`:

```bash
sha256sum --check --ignore-missing SHA256SUMS
gh attestation verify reasoning-engine-v0.2.0-linux-x86_64.tar.gz --repo Raoof128/reasoning-engine
tar -xzf reasoning-engine-v0.2.0-linux-x86_64.tar.gz reasoning-engine
```

Release binaries include the local HTTP server (`local-host`, off until you enable it) and signed, replayable run packs (`trust`).

### From source

Needs Rust 1.89 or newer (1.92 for `semantic`).

```bash
git clone https://github.com/Raoof128/reasoning-engine.git
cd reasoning-engine
cargo install --locked --path crates/re-cli                          # stdio MCP only
cargo install --locked --path crates/re-cli --features local-host,trust,semantic   # everything
```

### Docker

The image serves MCP over HTTP and always requires a bearer token:

```bash
docker build -t reasoning-engine .
docker run --rm -p 8765:8765 -v re-data:/data \
  -e REASONING_ENGINE_HTTP_TOKEN="$(openssl rand -hex 32)" reasoning-engine
```

Tagged releases also publish `ghcr.io/raoof128/reasoning-engine` for `linux/amd64` and `linux/arm64`. The image is distroless and runs as a non-root user. It signs run packs with `/data/signing.key`, created on first run, so keep the volume.

## Setup

```bash
reasoning-engine setup
```

`setup` asks where to keep data (default `~/.reasoning-engine`), which verifier to use, and whether to enable the local HTTP server and audit UI. Both are **off** unless you say yes. Every question has a flag (`setup --help`), and `setup` never prompts when stdin is not a terminal, so scripts can run it.

### Configure Claude Code

Add to your project's `.mcp.json`:

```json
{
  "mcpServers": {
    "reasoning-engine": {
      "command": "reasoning-engine",
      "args": ["mcp"]
    }
  }
}
```

Or register it for every project:

```bash
claude mcp add reasoning-engine -s user -- reasoning-engine mcp
```

Then copy the skill for the `/deep-research` slash command:

```bash
cp skill/deep-research.md ~/.claude/skills/
```

## Features

| Cargo feature | Adds | In release binaries |
|---|---|---|
| *(default)* | stdio MCP server (20 tools), `research`, `scholar search`, `setup`, `bench`, `verify-runpack` | yes |
| `local-host` | `serve` (Streamable HTTP with bearer auth) and the `/ui` audit UI | yes |
| `trust` | Signed run packs with a transparency log, `research --record` and `replay`, styled reference lists, the numeric claim check | yes |
| `semantic` | `hybrid` and `nli` verifier tiers (ONNX models, downloaded on first use) | no |
| `scholar-gateway` | Token-gated Scholar Gateway as an extra search source | no |

### Verifier tiers

| Tier | How it decides | Accuracy | Contradiction recall |
|---|---|---:|---:|
| `lexical` (default) | Word overlap | 0.585 | 0.00 |
| `hybrid` | BM25 + RRF shortlist, then BGE-small cosine | 0.565 | 0.00 |
| `nli` (recommended with `semantic`) | DeBERTa-v3 cross-encoder: entail / contradict / neutral | **0.912** | **0.98** |

The numbers come from `reasoning-engine bench verify` on 147 labelled claim/evidence pairs in `tests/fixtures/verify/labelled_pairs.json`. The labels were AI-written and have not yet been reviewed by a person. Lexical cannot detect contradiction at all, and embeddings cannot see negation, so `hybrid` mainly serves as NLI's shortlist. `high_stakes` runs always use `nli` when the build and models allow it. Models are pinned by revision and SHA-256 and checked on every load.

## Academic search and the verifiable pipeline

Search is live by default and fans out concurrently across seven free sources:

```bash
reasoning-engine scholar search "MCP prompt injection" --limit 3
```

The full pipeline searches, extracts claims, verifies them, runs the quality gate and exports an attested run pack:

```bash
reasoning-engine research "Retrieval augmented generation reduces hallucination" \
  --draft "Retrieval augmented generation reduces hallucination in large language models."
reasoning-engine verify-runpack ~/.reasoning-engine/runs/<run_id>
```

### Environment variables

| Variable | Effect |
|----------|--------|
| `REASONING_ENGINE_DB` | Database path (default `<data_dir>/reasoning.db`) |
| `REASONING_ENGINE_RUNS_DIR` | Run-pack directory (default `<data_dir>/runs`) |
| `REASONING_ENGINE_CONFIG` | Config file (default `~/.reasoning-engine/config.toml`) |
| `REASONING_ENGINE_VERIFIER` | `lexical`, `hybrid` or `nli` (overrides `setup`) |
| `REASONING_ENGINE_MODELS_DIR` | Where ONNX models are cached (default `<data_dir>/models`) |
| `REASONING_ENGINE_SIGNING_KEY` | Run-pack signing key (default `<data_dir>/signing.key`; `trust` builds) |
| `REASONING_ENGINE_OFFLINE=1` | Deterministic mock evidence; never downloads models |
| `REASONING_ENGINE_SOURCES=openalex,crossref` | Narrow which sources are enabled |
| `REASONING_ENGINE_CONTACT_EMAIL` | Contact address for OpenAlex/Crossref's polite pool |
| `REASONING_ENGINE_HTTP_TIMEOUT_SECS` | Per-request timeout (default 25) |
| `S2_API_KEY`, `OPENALEX_API_KEY` | Optional keys for higher rate limits |
| `REASONING_ENGINE_DORA=directional` | Add direction-level budget advice to `select_next_branches` |
| `REASONING_ENGINE_MEMORY=fts` | Rank memory recall with SQLite FTS5 BM25 |
| `SCHOLAR_GATEWAY_LIVE=1`, `SCHOLAR_GATEWAY_ACCESS_TOKEN` | Scholar Gateway (`scholar-gateway` builds) |

An empty value counts as unset. Tokens are never stored in SQLite or run packs.

## Trust and reproducibility (`trust`)

**Signed run packs.** Every export is signed with Ed25519 over its `run_pack_hash`, which already covers every artifact. The key is created on first use, with mode `0600`. Each signed export is also appended to a hash-chained transparency log in the database.

```bash
reasoning-engine keys show                      # your public key
reasoning-engine verify-runpack <pack> --public-key <base64>
```

`verify-runpack` checks the artifact hashes, the signature (pinned to a key when you pass one) and inclusion in the log. It exits:

- `0`: ok;
- `1`: tampered (an artifact, the signature, the key or the log disagrees);
- `2`: unknown (the hashes match, but the pack is unsigned).

Someone who edits a pack and re-signs it with their own key fails both the pinned key and the log.

**Replayable runs.**

```bash
reasoning-engine research --record "..." --draft "..."
reasoning-engine replay <run_id>     # offline; exits 1 if any artifact differs
```

A recorded run pins its clock and ID seed, and keeps every provider response in the pack (`http/` and `replay.json`). `replay` re-runs the pipeline with no network and compares every artifact hash with the attestation; a response that was not recorded fails closed.

Recordings never contain:

- API keys or your contact address (they are stripped from URLs before keying);
- secret response headers;
- any configured secret that a provider echoes back in a body.

Providers may still echo request metadata, though: DBLP's JSON includes the caller's IP address. So share recorded packs the way you would share request logs.

**References in your profile's style.** Packs gain `references.<style>.md`, rendered with CSL and listed in the attestation:

| Profile style | Rendered as |
|---|---|
| `harvard_au` | Harvard (Cite Them Right) |
| `vancouver` | Vancouver |
| `ieee` | IEEE |
| `chicago` | Chicago author-date |
| `legal` | AGLC4 |

`sources.bib` stays for compatibility.

**Numbers must match.** A quantitative claim cannot be `supported` unless one of its numbers appears in the evidence it cites:

- matching uses 2% relative tolerance and compatible units (`%`, percentage points, `ms`/`s`, `x`);
- ranges, confidence intervals and Persian digits are understood;
- years and confidence levels never count as the match.

Otherwise the claim is capped at `partially_supported` and flagged for review.

## Local HTTP server and audit UI

stdio is the default and needs no network. The HTTP server is opt-in (`local-host` feature, then `setup --http`, or `--http` on the command line):

```bash
reasoning-engine serve --http                 # http://127.0.0.1:8765/mcp
reasoning-engine serve --http --ui            # plus http://127.0.0.1:8765/ui/
```

Safety defaults:

- The server binds to loopback only, and accepts only loopback `Host` headers (DNS-rebinding protection).
- The MCP endpoint refuses any request that carries a browser `Origin` header, so web pages cannot drive the tools.
- A public bind needs `--unsafe-bind-public` **and** a bearer token, which `serve --bearer-token-env VAR` reads from the environment.
- Tokens are compared in constant time and never logged.
- Request bodies are capped at 1 MiB.

The audit UI is read-only and shows:

- recent sessions, each with its branch tree, scores and reflections;
- research runs, each with its claims and their verification status, evidence with risk flags, gaps and provenance.

When a token is set, the UI asks for it: the browser shows a sign-in prompt, and you enter the token as the password with any username. Pages carry a strict Content-Security-Policy with no scripts.

For Notion AI Custom MCP through a Cloudflare tunnel, run `./run-notion-mcp-laptop.sh` (or double-click `run-notion-mcp-laptop.command` on macOS). See [Notion Laptop MCP Tunnel](docs/notion-laptop-mcp.md).

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

Tool names, arguments, results and error messages match the Python server byte for byte after normalising IDs and timestamps. CI checks this on every change with `tests/parity/run_parity.py`.

## Migrating from Python

1. Install the binary (above).
2. In `.mcp.json`, change `command` from `/path/to/.venv/bin/reasoning-engine` to `reasoning-engine`. The arguments stay `["mcp"]`.
3. Keep your data. The Rust engine opens the Python database unchanged. Python's default location was inside the package (`src/reasoning_engine/reasoning.db`), so either move that file to `~/.reasoning-engine/reasoning.db` or set `REASONING_ENGINE_DB` to its path.

`serve --transport http` still works as an alias for `serve --http`. The Python package (`pip install -e .`, `Dockerfile.python`) stays for 0.2.x with a deprecation warning, and is removed in 0.3.0.

## Project Structure

```
reasoning-engine/
  crates/
    re-core/        # Pure algorithms: difficulty, DORA, UCB1, sanitiser, ranking
    re-model/       # Records shared by storage and verification
    re-store/       # SQLite (Python-compatible schema), FTS5 memory recall
    re-retrieval/   # Seven academic adapters + concurrent aggregator
    re-verify/      # Claims, verifier tiers, quality gate, run packs
    re-mcp/         # The 20 MCP tools; stdio and Streamable HTTP
    re-ui/          # Read-only audit UI (local-host)
    re-cli/         # The reasoning-engine binary
  tests/            # Golden vectors, recorded HTTP, parity harness, labelled pairs
  src/              # Deprecated Python implementation (removed in 0.3.0)
  skill/            # Claude Code skill file
```

## Background

- **Process Reward Models** score individual reasoning steps rather than only final answers, which gives tree search dense feedback.
- **DORA (Direction-Oriented Resource Allocation)** uses score variance to switch dynamically between exploring many paths and exploiting the best one.
- **Reflexion** feeds textual critiques back into the prompt, so the model can self-correct without weight updates.
- **UCB1 selection** balances trying promising branches against exploring under-visited ones.
- **ReAct / Self-RAG style evidence checks** keep retrieval and verification explicit before synthesis.

## Documentation

- [Architecture Overview](docs/architecture.md)
- [API Reference](docs/api-reference.md)
- [Usage Examples](docs/examples.md)
- [Rust rewrite plan](docs/rust-rewrite-plan-2026.md)
- [Contributing](CONTRIBUTING.md)
- [Security Policy](SECURITY.md)
- [Code of Conduct](CODE_OF_CONDUCT.md)

## License

MIT
