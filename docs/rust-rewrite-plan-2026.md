# Rust Rewrite Plan (October 2026)

Status: proposal, researched against docs current as of 2026-10-18.
Scope: port the `reasoning-engine` MCP server from Python to Rust, keep the
`/deep-research` skill and every tool name unchanged, and use the port to fix
the accuracy ceilings the Python MVP documents about itself.

## 1. TL;DR

- **Do it as a 7-crate Cargo workspace** on Rust stable (1.99 today, MSRV
  1.89 for the default build, set by uuid; `semantic` needs 1.92 for ort and
  `wasm-adapters` needs 1.97 for wasmtime) / edition 2024, using the official `rmcp` 3.5.1 SDK (which
  implements MCP `2026-07-28`), `tokio`, `reqwest`, `rusqlite` (bundled, WAL),
  `schemars` for tool schemas.
- **Speed wins are real but specific.** Hypothesis to be measured in Phase 1
  (`hyperfine`, `criterion`): roughly 10x faster cold start, 5-10x lower RSS,
  and much better fan-out behaviour (per-source deadlines, connection reuse,
  backpressure). The deterministic maths is already fast in Python; the slow
  parts are process start, blocking `urllib` fan-out with a 25 s timeout, and
  opening a fresh SQLite connection per call. No number here is a promise
  until the bench job prints it.
- **Executable plans live next to this one.**
  `docs/superpowers/plans/2026-10-18-rust-port-phases-0-6.md` and
  `docs/superpowers/plans/2026-10-18-rust-beast-phases-7-12.md` are the
  task-by-task versions an agent runs. This document is the rationale.
- **Accuracy is where the big upgrade lives.** The current verifier is lexical
  overlap and can never emit `contradicted`. The Rust port should ship a tiered
  verifier: BM25 + embeddings to shortlist evidence, then an NLI cross-encoder
  (ONNX, CPU) to label entailment / contradiction / neutral. Retrieval ranking
  moves from a hand-weighted blend to Reciprocal Rank Fusion with retraction
  checks. DORA moves from a max-min "kappa" to direction-level allocation as in
  the paper it is named after.
- **Parity first.** `tests/golden/golden_vectors.json` (generated in this
  branch) freezes the Python behaviour of difficulty, DORA, UCB, sanitizer,
  relevance scoring and DOI normalisation. The Rust core must reproduce it
  bit-for-bit before anything is "improved".
- **Local host is opt-in.** Stdio is the only thing that runs by default. The
  local HTTP MCP server, the embedded audit UI and the Notion tunnel are
  compiled behind a `local-host` feature and enabled only when the user says
  yes in `reasoning-engine setup` (section 3.3).
- **Beast mode is wired in as phases 7-12.** Every item from
  `beast-mode-vision-2026.md` has a phase, an exit criterion and a crate
  (section 8). Nothing from the vision ships before the parity gate.

## 2. What the 2026 docs say

### 2.1 Toolchain and crates (verified on crates.io / releases.rs, 2026-10-18)

| Component | Version | Date | Why it matters here |
|---|---|---|---|
| Rust stable | 1.99.1 (beta 1.100 due 12 Nov 2026) | Oct 2026 | Edition 2024: async closures, `AsyncFn` in prelude, RPIT capture rules. `cargo 1.97` is installed in this container. |
| `rmcp` (official MCP SDK) | 3.5.1, MSRV 1.88 | 2026-10-05 | Implements MCP 2026-07-28, stays compatible with 2025-11-25 and earlier. Stdio via `transport-io`; Streamable HTTP as a Tower service via `transport-streamable-http-server`. API moved 3.3 -> 3.5 within one month, so pin exactly. |
| `tokio` | 1.53.3 | 2026-10-13 | Runtime. |
| `reqwest` | 0.13.5 | 2026-09-08 | HTTP client with pooling, HTTP/2, per-request timeouts. Features verified on crates.io: `default-features = false, features = ["rustls", "json", "http2", "gzip"]` (the feature is `rustls`, not `rustls-tls`). |
| `indexmap` | 2.14.2 | 2026-09-05 | Insertion-ordered maps so DORA/UCB tie-breaking matches Python dict order. |
| `subtle` | 2.6.1 | 2024-06-24 | Constant-time bearer token comparison. |
| `hyperfine` (tool) | 2.0.0 | 2026-10-07 | Cold-start benchmark Python vs Rust. |
| `rusqlite` | 0.40.2 | 2026-08-08 | Mature SQLite binding, `bundled` feature pins the SQLite version. Turso's Rust-native engine (0.7.x) is still pre-1.0 and explicitly warns API may change, so not yet. `sqlx` 0.9 is fine but adds a compile-time query checker we do not need. |
| `axum` / `tower-http` | 0.8.9 / 0.7.1 | 2026 | Mount the rmcp Tower service, add bearer auth middleware, body limits. |
| `schemars` + `serde` + `serde_json` | 1.2.3 / 1.0.229 / 1.0.154 | 2026 | JSON Schema for `inputSchema` and `outputSchema`. |
| `fastembed` | 7.1.1 | 2026-10-07 | Local ONNX embeddings and cross-encoder reranking, user-defined model loading. Built on `ort`. |
| `ort` | 2.0.0-rc.14 (no stable yet) | 2026-10-12 | ONNX Runtime binding. Pre-release; isolate behind a feature flag. |
| `tantivy` | 0.27.0 | 2026-10-17 | In-memory BM25 for evidence and memory recall. |
| `quick-xml` | 0.42.0 | 2026-08-22 | arXiv Atom parsing. No DTD/entity expansion by default (replaces `defusedxml`). |
| `proptest` / `insta` / `criterion` | 1.12.0 / 1.49.0 / 0.8.2 | 2026 | Property tests, snapshot tests, benchmarks. |
| `cargo-nextest` / `cargo-deny` / `cargo-mutants` | 0.9.148 / 0.20.2 / 27.1.0 | 2026 | Test runner, supply-chain policy, mutation testing. |
| `clap`, `thiserror`, `anyhow`, `tracing`, `uuid`, `sha2`, `governor` | 4.6.7 / 2.0.21 / 1.0.104 / 0.1.44 / 1.28.0 / 0.11.1 / 0.10.4 | 2026 | CLI, errors, telemetry, IDs, hashing, rate limiting. |
| `inquire` | 0.9.4 | 2026-02-24 | Interactive prompts for `reasoning-engine setup` (TTY only; falls back to flags). |
| `hayagriva` | 0.10.1 | 2026-06-14 | CSL citation rendering (Harvard AU, Vancouver, IEEE, Chicago). Phase 7. |
| `ed25519-dalek` | 3.0.0 | 2026-07-06 | Signed attestations and Merkle transparency log. Phase 7. |
| `nom` | 8.0.0 | 2025-01-26 | Numeric claim grammar (numbers, units, ranges, CIs). Phase 7. |
| `petgraph` | 0.8.3 | 2025-09-30 | Evidence graph, corroboration independence, retraction cascade. Phase 10. |
| `usearch` | 2.26.5 | 2026-10-18 | Single-file HNSW index for chunks and cross-session evidence memory. Phases 9-11. |
| `pdfium-render` / `pdf-extract` | 0.9.4 / 0.12.1 | 2026-09 | Full-text extraction of open-access PDFs. Phase 9. |
| `linfa` | 0.8.1 | 2025-12-23 | Learned PRM prior trained on the engine's own history. Phase 11. |
| `askama` | 0.16.1 | 2026-09-04 | Embedded audit UI templates, `local-host` feature only. Phase 6. |
| `wasmtime` | 50.0.0-rc.1 (pre-release) | 2026-10-14 | Sandboxed WASM source adapters. Phase 12, optional. |
| `wiremock` | 0.6.5 | 2025-08-24 | Recorded-HTTP adapter tests. Phase 3. |

### 2.2 MCP specification 2026-07-28: changes that affect this server

Source: official changelog (modelcontextprotocol.io/specification/2026-07-28/changelog).

| Change | Impact on the rewrite |
|---|---|
| Protocol is stateless; `initialize` handshake removed; version and capabilities ride in `_meta`; new mandatory `server/discover` RPC. | rmcp 3.x handles this. Our tools must not rely on per-connection state: session and run IDs are already explicit handles passed as arguments, which is exactly the pattern SEP-2567 prescribes. |
| `Mcp-Session-Id` and SSE resumability removed from Streamable HTTP; one POST endpoint; broken streams are re-issued by the client. | Long-running `run_research_pipeline_tool` must be idempotent per `run_id` (it is not today: a retry creates a second run). |
| Tasks move to an official extension with `tasks/get` polling. | Expose the full pipeline as a task so a 20-60 s academic fan-out does not sit on one HTTP response. |
| MRTR replaces server-initiated elicitation/sampling. Roots, Sampling, Logging deprecated. | We never used them. Log to stderr (stdio) and OpenTelemetry via `tracing`. |
| `tools/list` SHOULD be deterministic; results carry `ttlMs` and `cacheScope`. | Register tools in a fixed order; advertise a long TTL (the tool list is static). |
| `inputSchema`/`outputSchema` accept full JSON Schema 2020-12; `structuredContent` any JSON. | Return typed `Json<T>` outputs with `outputSchema` instead of JSON-in-a-string, which is what every tool does today. |
| Required `resultType` on every result. | rmcp handles. |
| Error code allocation policy (`-32020..-32099` reserved for MCP). | Map our validation errors into the implementation-defined range `-32000..-32019`. |
| OpenTelemetry `traceparent` in `_meta`. | Propagate into `tracing` spans so a run's provenance ledger can carry trace IDs. |

### 2.3 Academic source APIs

- **OpenAlex** (help.openalex.org, last updated 2026-10-15): keys are *not*
  mandatory for basic queries, contrary to some secondary reporting. Limits are
  10 req/s keyless, 30 req/s with a free key; a free key gives 10x the daily
  budget and is required for cursor paging. Cost is reported per response in
  `X-RateLimit-Credits-Used`. The `mailto` polite-pool parameter is no longer
  documented on that page. Action: add `OPENALEX_API_KEY`, read the credits
  header into provenance, and treat a missing key as a soft warning.
- **Semantic Scholar**: the public docs landing page does not state limits; the
  existing `S2_API_KEY` support should stay and the adapter should back off on
  429 using `Retry-After`.
- **arXiv, Crossref, Europe PMC, DOAJ, DBLP**: no 2026 policy changes found.
  Crossref still rewards a descriptive `User-Agent` with a contact address.

### 2.4 Research that should shape the algorithms

- DORA is from Wang et al. (2025) "Every Rollout Counts". Its point is to
  allocate rollouts per *direction* (cluster of similar reasoning paths),
  decoupled from how many candidates each direction already has. Our
  implementation uses `kappa = max - min` of branch scores as a breadth/depth
  switch, which is a budget heuristic, not DORA. Section 6.3 proposes the fix.
- PRM survey (Zheng et al. 2025) and Chen et al. (2025) on compute scaling:
  Best-of-N is the right default under tight budgets, MCTS/beam only when
  compute is plentiful. Our difficulty tiers already encode that; the data to
  calibrate the thresholds is sitting unused in `episodic_memory`.
- CATS (Song et al. 2025, arXiv preprint; a secondary source labelled it ICLR
  2026 but the arXiv page lists no venue) allocates per-step compute using a
  learned controller. Out of scope for a deterministic server, but it argues
  for logging per-step score variance so a future controller has training
  data.

## 3. Target architecture

```
reasoning-engine/            (Cargo workspace, edition 2024, MSRV 1.89)
  crates/
    re-core/        pure, no I/O:  difficulty, dora, ucb, sanitizer, research
                    planning, claim typing, quality gate, newtypes (Score, Depth)
    re-store/       rusqlite: schema v3 migrations, sessions, branches,
                    reflections, memory, verifiable ledger, provenance
    re-retrieval/   reqwest adapters (7 sources + Scholar Gateway), aggregator
                    with RRF, dedup, retraction flags, governor rate limits
    re-verify/      claim extraction, tiered verifier (lexical | bm25+embed |
                    nli), run-pack export + attestation
    re-mcp/         rmcp 3.5 server: tool structs, stdio (always) + streamable
                    HTTP (feature `local-host`), bearer auth, bind safety,
                    tasks extension for speculative retrieval
    re-ui/          feature `local-host` only: axum + askama audit UI
                    (branch tree, kappa over rounds, evidence graph)
    re-cli/         clap binary `reasoning-engine` (setup | mcp | serve |
                    research | scholar search | verify-runpack | replay |
                    watch | calibrate | bench)
  skill/deep-research.md     unchanged
  tests/golden/              Python-generated parity vectors (this branch)
  tests/parity/              harness: same tool calls -> Python and Rust -> diff
```

Cargo features (defaults in bold): **`stdio`**, `local-host` (HTTP server,
audit UI, tunnel helper), `semantic` (ort, fastembed, NLI), `fulltext`
(pdfium), `fa` (Persian sources and multilingual embeddings), `wasm-adapters`.
The default build is stdio plus the lexical verifier and has no native
dependencies beyond bundled SQLite.

Design rules:

- `re-core` has zero dependencies on tokio, reqwest or rusqlite. It compiles to
  wasm as a side effect, which keeps it honest.
- Every score is a `Score(f64)` newtype that can only be built in `[0, 1]`.
  Branch and session states are enums, not strings. The MCP boundary parses
  into these types once; nothing downstream re-validates.
- Verification backends sit behind a `Verifier` trait with three impls:
  `LexicalOverlap` (parity with Python, always available), `Hybrid` (BM25 +
  embeddings), `Nli` (cross-encoder). Selected by `REASONING_ENGINE_VERIFIER`.
- `ort`-dependent code is behind a `semantic` Cargo feature so the default
  build stays small and the pre-release dependency cannot break `cargo build`.

### 3.1 Module mapping

| Python | Rust crate::module | Notes |
|---|---|---|
| `difficulty.py` | `re_core::difficulty` | Parity via golden vectors. Add feature logging for calibration. |
| `dora.py` | `re_core::dora` | Parity first, then `dora::directional` (6.3). |
| `ucb.py` | `re_core::ucb` | `f64::INFINITY` for unvisited, same as Python. |
| `sanitizer.py` | `re_core::sanitize` | `regex` crate; add NFKC + confusables (section 7). |
| `research.py` | `re_core::planning` | Angle library + gap questions. |
| `validation.py` | `re_mcp::params` | Becomes serde + schemars types; limits as `const`. |
| `db.py`, `sessions.py`, `memory.py` | `re_store::{schema, sessions, memory}` | One connection per thread, WAL. Migrations are additive only (new tables/indexes via `IF NOT EXISTS`), so a database the Python code created opens unchanged. `user_version` is advisory because Python `init_db` rewrites it to 2 on every start; detect schema by table presence. |
| `verifiable/sources/*.py` | `re_retrieval::sources::*` | One file per provider, trait `SourceAdapter`. |
| `verifiable/aggregator.py` | `re_retrieval::aggregate` | `tokio::JoinSet`, per-source deadline, RRF. |
| `verifiable/claims.py`, `quality.py` | `re_verify::{claims, gate}` | Tiered verifier. |
| `verifiable/runpack.py` | `re_verify::runpack` | Same artifact names and hashes so old packs still verify. |
| `server.py`, `transport.py` | `re_mcp::{tools, http}` | 20 tools, identical names and argument names. |
| `cli.py` | `re_cli` | Same subcommands plus `verify-runpack` and `bench`. |

### 3.2 Minimal rmcp 3.5 tool shape

```rust
#[derive(Deserialize, JsonSchema)]
pub struct InitResearchSession { pub query: String }

#[derive(Serialize, JsonSchema)]
pub struct SessionCreated {
    pub id: String, pub difficulty: f64, pub strategy: Strategy, pub budget: Budget,
}

#[tool_router]
impl ReasoningEngine {
    #[tool(name = "init_research_session",
           description = "Create a research session, estimate difficulty, allocate budget",
           annotations(read_only_hint = false, idempotent_hint = false))]
    async fn init_research_session(
        &self, Parameters(p): Parameters<InitResearchSession>,
    ) -> Result<Json<SessionCreated>, McpError> {
        let query = BoundedText::new(p.query, limits::MAX_QUERY_CHARS)?; // our newtype
        Ok(Json(self.sessions.create(query).await?))
    }
}
```

Verified against rmcp 3.5 docs: `#[tool(... annotations(title = "...",
read_only_hint = true))]` is the real syntax; `Json<T>` puts the value in
`structured_content` with an `outputSchema`; `McpError` is rmcp's error type.
`BoundedText` and `limits` are ours. For the tasks extension (Phase 8) rmcp
ships `rmcp::task_manager::TaskManager` (`spawn`, `tasks/get`,
`tasks/update`, `tasks/cancel`, TTL expiry) and tools return
`CallToolResponse::Task(CreateTaskResult)`.

Each tool lives in its own file and is registered with
`ToolRouter::new().with_async_tool::<T>()` in a fixed order, as the rmcp docs
recommend once a server grows past a handful of tools.

### 3.3 Optional local host: HTTP server, audit UI, tunnel

Nothing listens on a port unless the user asks. Only the `setup` subcommand
ever prompts; every other subcommand prints a single stderr hint
("no config found, run `reasoning-engine setup`") and proceeds with defaults.
`setup` asks:

```
reasoning-engine setup
  Data directory ........................ ~/.reasoning-engine   [Enter to accept]
  Verifier tier ......................... lexical / hybrid (130 MB) / nli (+90 MB)
  Set up a local HTTP MCP server? ....... No / Yes
    Bind address ........................ 127.0.0.1 only (public bind needs --unsafe-bind-public)
    Port ................................ 8765
    Generate a bearer token? ............ Yes -> ~/.reasoning-engine/http.env (0600)
  Enable the local audit UI? ............ No / Yes (served at http://127.0.0.1:8765/ui)
  Set up the Notion/Cloudflare tunnel? .. No / Yes (needs cloudflared on PATH)
  Register with Claude Code now? ........ Yes -> runs `claude mcp add reasoning-engine -s user -- <binary> mcp`
```

Rules:

- Stdio MCP never asks anything: when stdin is not a TTY, `setup` is skipped
  and defaults apply. The MCP protocol owns stdin, so prompts there would
  corrupt the stream.
- Answers persist to `~/.reasoning-engine/config.toml`. Every question has a
  flag equivalent (`--http`, `--port`, `--ui`, `--tunnel`, `--verifier`) and an
  environment variable, so CI and Docker never see a prompt.
- `serve` refuses to start when `http.enabled = false` in config and no
  `--http` flag is given, with a one-line hint pointing at `setup`.
- The audit UI binds to the same loopback listener as the MCP endpoint, under
  `/ui`, protected by the same bearer token, read-only, no JavaScript build
  step (`askama` templates, inline SVG for the tree).
- Builds without the `local-host` feature have no `serve`, `ui` or `tunnel`
  subcommands at all, and the axum/hyper server stack is not compiled in.
- `setup` is re-runnable and idempotent; `setup --reset` wipes config but
  never the database or run packs.

## 4. Speed: what actually gets faster

| Hot spot in Python today | Rust change | Expected effect |
|---|---|---|
| Interpreter + `mcp` SDK import on every stdio launch (hundreds of ms). | Single static binary. | Cold start in low tens of ms. Measure with `hyperfine`. |
| `ThreadPoolExecutor` over blocking `urllib`, 25 s timeout per source, no connection reuse, no HTTP/2. | `tokio::JoinSet` fan-out, `reqwest` pool with keep-alive, per-source deadline (e.g. 8 s) plus an overall "first 5 sources or 6 s" policy, `governor` token buckets per host. | p50 search latency bounded by the fast sources rather than the slowest; fewer 429s. |
| New `sqlite3.connect` per tool call, PRAGMAs re-applied each time. | Long-lived connection(s), prepared-statement cache, single write transaction per tool call. | Sub-millisecond tool calls for session/branch operations. |
| JSON round-trips: tools return JSON as a string that the client parses again. | `structuredContent` with `outputSchema`. | Fewer bytes, fewer parses, typed on both sides. |
| `recall_memory` loads every row and scores in Python. | FTS5 (bundled SQLite) for the lexical path; BM25/embedding path optional. | Constant time per query as memory grows. |
| Claude waits on every `scholar_search_tool` call in sequence. | **Speculative retrieval (Phase 8):** `plan_research_angles_tool` returns a task handle and the server fans out all angles at once into a content-addressed cache keyed by `(source, normalised query)`. | By the time Claude registers branch one, angles two to six are cached, deduped and ranked. |
| Every test and bug report needs live network. | **Replayable runs (Phase 7):** HTTP responses recorded into the run pack, content-addressed; `reasoning-engine replay <run_id>` reproduces the pipeline offline, byte for byte. | Deterministic tests, reproducible attestations, offline demos. |

Honest framing: for an end-to-end `/deep-research` run the wall clock is
dominated by Claude's own reasoning and by remote API latency. The Rust port
makes the server negligible, predictable and cheap to run, and it unlocks the
semantic verifier, which Python could also run but at a far higher memory and
startup cost.

Benchmarks to commit under `crates/*/benches` with `criterion` and a
`reasoning-engine bench` subcommand: difficulty estimation, DORA selection on
8 branches, sanitizer on 50 KB of HTML, aggregator against a local mock HTTP
server with injected latency, verifier on 20 claims x 50 snippets.

## 5. Quality: how the port earns trust

1. **Golden parity.** `tests/golden/golden_vectors.json` is loaded by
   `re-core` tests; every vector must match to the last decimal. Regenerate only
   with an explicit decision to change behaviour.
2. **Property tests (`proptest`).** Difficulty is always in `[0, 1]` and
   non-decreasing when words that are neither keywords nor conjunctions are
   appended; DORA's three output lists are pairwise disjoint and together
   cover every input branch; `budget_remaining` never goes negative;
   `sanitize` output contains no `<`-delimited tags, a single injection
   phrase is always removed, and output length in characters never exceeds
   input length (each `[t](u)` becomes the shorter `t (u)`); `normalize_doi`
   is idempotent. "Output matches no injection pattern" is **false**: nested
   payloads survive (section 7). `sanitize` is **not** idempotent (a second pass decodes
   entities the first pass produced, verified: `A &amp;lt;b&amp;gt;` becomes
   `A &lt;b&gt;` then `A`); preserve that for parity, document it, and
   never call it twice on the same text.
3. **Snapshot tests (`insta`)** for every tool's JSON output on a fixed
   fixture database, so schema drift is a reviewed diff.
4. **Parity harness.** `tests/parity/run.sh` drives the same scripted tool
   sequence through the Python server and the Rust server over stdio and diffs
   the JSON (ignoring IDs and timestamps). This is the cut-over gate.
5. **Mutation testing** with `cargo-mutants` on `re-core` and `re-verify` in a
   weekly CI job; target no surviving mutants in scoring code.
6. **Supply chain.** `cargo-deny` (advisories, licences, duplicate bans),
   `cargo audit`, `#![forbid(unsafe_code)]` in every crate, Dependabot for
   Cargo. `clippy::pedantic` warnings as errors in CI.
7. **CI matrix.** Linux, macOS, Windows; `cargo nextest run`; `cargo fmt
   --check`; `cargo doc` with broken-link denial; a release job that publishes
   static binaries and a distroless Docker image.

## 6. Accuracy upgrades (the actual point)

### 6.1 Verifier: from term overlap to entailment

Today `verify_claims` uses the Jaccard-ish overlap of >3-letter tokens, with
thresholds 0.65 / 0.35. It cannot distinguish "X reduces hallucination" from
"X does not reduce hallucination", and `contradicted` is a status the gate
checks for but nothing ever produces.

Proposed tiers, chosen per run by `REASONING_ENGINE_VERIFIER` and by mode
(`high_stakes` forces `nli`):

| Tier | Mechanism | Latency (CPU) | Model footprint |
|---|---|---|---|
| `lexical` | Current algorithm, kept for offline/CI and as parity baseline. | microseconds | none |
| `hybrid` | `tantivy` BM25 over snippets + `fastembed` `BAAI/bge-small-en-v1.5` cosine, fused with RRF to shortlist the top 5 snippets per claim. | ~5 ms per claim | ~130 MB cached |
| `nli` | Shortlist from `hybrid`, then an NLI cross-encoder (`cross-encoder/nli-deberta-v3-xsmall`, 87.8 MNLI-mm accuracy; or `nli-MiniLM2-L6-H768`) exported to ONNX with `optimum-cli export onnx`, run through `ort`. Three logits: contradiction / entailment / neutral. | ~20-40 ms per pair | ~90 MB |

Mapping to existing statuses, so the quality gate and run-pack schema do not
change:

- entailment p >= 0.7 -> `supported`
- contradiction p >= 0.7 -> `contradicted` (new in practice; blocks the gate)
- entailment in [0.4, 0.7) -> `partially_supported`
- otherwise -> `unsupported`, with `needs_more_evidence` when the shortlist is empty

Implementation notes: `fastembed::TextRerank` reads only the first logit column,
so the NLI head needs a thin `ort` session of our own (same tokenizer plumbing
via `UserDefinedRerankingModel` types, different output handling). Record the
model name, SHA-256 and label order in `methodology.md` and in the attestation
so run packs stay reproducible. Models are downloaded once to
`~/.reasoning-engine/models` with hash verification; `REASONING_ENGINE_OFFLINE=1`
must never trigger a download.

### 6.2 Retrieval ranking and hygiene

- Replace `0.6 * position + 0.4 * log(citations)` with **Reciprocal Rank
  Fusion** (k = 60) across sources, then apply profile priors: recency weight
  for `fast` profiles, source-type preference from `ResearchProfile`.
- **Retraction and integrity flags:** OpenAlex `is_retracted`, Crossref
  `update-to` (retraction notices) populate `risk_flags`, and retracted items
  can never be `supported` evidence.
- **Dedup:** DOI first, then arXiv ID to DOI mapping (every arXiv paper has
  the DataCite DOI `10.48550/arXiv.<id>`, and the Atom feed carries an
  `arxiv:doi` element when a journal DOI exists), then fuzzy title match
  (normalised Levenshtein <= 0.08) so preprint and published versions merge
  instead of double counting.
- **Per-source budgets:** request `limit * 2` from each source, fuse, cut to
  `limit`; sources that time out are recorded as gaps with their latency.
- **Observability:** every adapter call logs status, latency, credits header
  (OpenAlex) and `Retry-After` into provenance.

### 6.3 Planner: implement DORA as published

Keep the current `select_branches` behind `dora::legacy` for parity. Add
`dora::directional`:

1. Group branches into directions by the research angle they came from (the
   planner already assigns angles) or, with the `semantic` feature, by
   embedding similarity of their traces.
2. Direction quality = mean of top-2 scores in the direction, so a direction
   with many weak candidates is not favoured for its count.
3. Allocate the remaining step budget across directions by softmax of
   direction quality with a temperature tied to difficulty (hard query, flatter
   allocation).
4. Within a direction pick the next branch by UCB1 (already implemented) with
   progressive widening: a node may gain a new child only while
   `children < ceil(C * visits^alpha)` with `C = 1.0`, `alpha = 0.5`.
5. Reflexion and pruning thresholds stay as they are; both are per-branch.

The skill file needs no change: `select_next_branches` returns the same
fields plus an optional `directions` array.

### 6.4 Difficulty estimator

Keep deterministic. Add: unique-token ratio, named-entity-ish capitalised
spans, numeric constraint count, and a per-profile prior (medicine/law start
higher). Log the feature vector and the eventual steps used into
`episodic_memory` so thresholds can be recalibrated from real sessions; a
`reasoning-engine calibrate` subcommand prints the confusion of predicted tier
against steps actually consumed.

### 6.5 Memory recall and claim extraction

- Memory: SQLite FTS5 (bundled) with BM25 as the default path, embeddings as
  the `semantic` path, fused by RRF. Replaces `set(words) & set(words)`.
- Claims: use `unicode-segmentation` sentence boundaries instead of a regex
  split, keep the deterministic claim typing, and add a `numeric_span` field
  so quantitative claims can be matched on their numbers.

### 6.6 Numeric claim checker (Phase 7)

A `nom` grammar parses numbers, percentages, ranges, confidence intervals,
comparatives and units from both the claim and the evidence. Units are
normalised, values matched within tolerance. A quantitative claim whose
number appears nowhere in its supporting evidence cannot be `supported`,
whatever the NLI model says.

### 6.7 Adversarial branch (Phase 10)

`beam_search` and `forest` reserve one branch for the counter-position. The
server generates negated queries (`"X does not reduce Y"`, `"replication
failure X"`) and the NLI tier scores contradiction rather than support. A
claim that survives a funded contradiction search earns a confidence bonus;
one that does not gets a steel-manned counter-view section in the report.

### 6.8 Evidence graph and corroboration independence (Phase 10)

Build a per-run `petgraph` of evidence nodes with citation and shared-author
edges from OpenAlex `referenced_works` and `authorships`. Each claim gets an
**independence score** (connected components among its supporters). A
retraction propagates: claims whose only support is downstream are demoted
and the run pack gains `cascade.json`. The report embeds a Mermaid snippet
per claim.

### 6.9 Full-text verification (Phase 9)

Pull open-access full text (OpenAlex `oa_url`, Europe PMC XML), extract with
`pdfium-render` or `pdf-extract`, chunk by section into `usearch`, and verify
claims against the body of the paper. The NLI tier receives the section label
as context; support found only in an introduction is `partially_supported`
with the reason stated.

### 6.10 Calibrated confidence (Phase 10)

Conformal prediction over a few hundred labelled claim-evidence pairs built
from past run packs turns NLI scores into "supported at 90% coverage" sets.
No crate needed; sorted residuals and a quantile.

### 6.11 Learned PRM prior (Phase 11)

Traces, critiques, score deltas and gate outcomes already form a labelled
dataset. `fastembed` embeds traces, `linfa` fits a small classifier predicting
consensus membership. It seeds UCB `q_value` instead of 0.0, flags large
disagreement with Claude's self-score for reflexion, and sharpens the
difficulty estimator. Blend weight grows with data volume; cold start is the
deterministic heuristic; models are versioned artifacts next to the database.

### 6.12 Source-quality model and cross-lingual research (Phases 9 and 12)

Europe PMC publication types and OpenAlex venue, open-access and institution
data score evidence against each profile's `preferred_source_types` instead
of a constant label. Behind the `fa` feature, `bge-m3` multilingual
embeddings let a Persian query find English evidence and vice versa, with
adapters for SID, Magiran and ISC, and reports rendered in either language.

## 7. Security posture carried over and tightened

- **Sanitizer nested-payload bypass (found in Phase 1, affects the live Python
  code).** Each injection pattern is applied once, so
  `ignore previous ignore previous instructions instructions` sanitizes to
  `ignore previous instructions`. **Fixed in both implementations** on
  `rust/phase-1-core`: the injection pass repeats until the text stops
  changing, bounded at 8 passes plus one final pass, with four nested golden
  vectors. **Residual:** nesting ten or more levels deep still survives
  (verified). Unbounded repetition would close it and always terminates,
  because every pattern removes at least one character per match; the cost
  is quadratic only on adversarial input. Decide before Phase 4 exposes the
  tool.
- Sanitizer: keep all current regexes; add NFKC normalisation before matching,
  strip Unicode confusables used to smuggle "ignore previous instructions", and
  bound input length before regex execution (ReDoS guard). Add a
  `risk_flags` output rather than silently deleting matches, so the skill can
  show what was removed.
- Streamable HTTP: rmcp's `StreamableHttpServerConfig` has `allowed_hosts`,
  `allowed_origins` and `max_request_body_bytes`; set them (DNS-rebinding
  protection equivalent to the FastMCP 1.23 floor the Python code enforces).
  Keep `--unsafe-bind-public` semantics and the constant-time bearer check
  (`subtle::ConstantTimeEq`).
- SSRF: adapters may only hit their fixed hosts; no URL comes from tool input.
- XML: `quick-xml` with entity expansion disabled and a size cap.
- SQLite: `0o600` on creation, `PRAGMA foreign_keys`, `trusted_schema = OFF`.
- Run packs: Ed25519 signature (`ed25519-dalek` 3.0.0) on `attestation.json`
  so the `signature` and `signature_algorithm` fields stop being `null`, plus
  an append-only Merkle log of run-pack hashes in SQLite.
  `reasoning-engine verify-runpack` checks signature, inclusion proof and
  artifact hashes offline (Phase 7).
- Recorded HTTP for replay strips `Authorization`, `Set-Cookie` and any
  header named in a deny-list before it touches disk.
- Local host (when enabled): loopback bind by default, bearer token file at
  `0600`, audit UI read-only, `allowed_hosts`/`allowed_origins` set, body
  limit set, tunnel only via the explicit helper.
- WASM adapters (Phase 12): no filesystem, no environment, outbound HTTP only
  to the adapter's declared host list, fuel-metered execution.

## 8. Migration phases and exit criteria

| Phase | Work | Exit criterion |
|---|---|---|
| 0 (done in this branch) | Golden vectors generated from Python. | `tests/golden/golden_vectors.json` committed. |
| 1 | Workspace skeleton, `re-core` with all pure algorithms, golden + property tests, CI. | `cargo nextest run` green on 3 OSes; golden parity 100%. |
| 2 | `re-store` with schema v3 migration that opens an existing `reasoning.db` unchanged. | Python test fixtures load; `pytest` suite's DB assertions re-expressed in Rust. |
| 3 | `re-retrieval`: 7 adapters + aggregator, recorded-HTTP fixtures (`wiremock`). | Same evidence set as Python on recorded responses, modulo new RRF ordering (documented). |
| 4 | `re-mcp` + `re-cli`: all 20 tools over stdio; `setup` wizard with config file and flag equivalents; HTTP server and bearer auth behind `local-host`, off by default. | Parity harness diff is empty for the scripted `/deep-research` flow. `setup` never runs when stdin is not a TTY. |
| 5 | `re-verify` tiers `hybrid` and `nli` behind `semantic`; DORA directional; FTS5 memory. | A labelled set of 100 claim/evidence pairs (hand-built from existing run packs) shows NLI beating lexical on contradiction recall; lexical remains default-off only in `high_stakes`. |
| 6 | Cut-over: README, Dockerfile (distroless, `FROM scratch` + CA certs, built with `--features local-host` and started with `--http` because the image's only job is to serve HTTP), `.mcp.json` examples; `re-ui` audit UI under `local-host`; Python marked deprecated for one release, removed next. | Users swap the `command` line only. UI reachable only when `setup` enabled it. |
| 7 | **Trust and reproducibility:** replayable runs (recorded HTTP), Ed25519-signed attestations + Merkle log, `verify-runpack`, CSL citations via `hayagriva`, numeric claim checker. | `replay` reproduces an attested pack byte for byte offline; a tampered pack fails `verify-runpack`; `sources.bib` renders in the profile's style. |
| 8 | **Speculative retrieval** via the MCP tasks extension; content-addressed evidence cache; cost ledger (credits, bytes, estimated tokens). | Angles 2..N are cached before Claude registers branch 1 in the parity flow; ledger appears in provenance. |
| 9 | **Full-text verification** (`fulltext` feature) with section-aware chunks in `usearch`; source-quality scoring from Europe PMC and OpenAlex metadata. | On the Phase 5 labelled set, full-text NLI beats abstract-only NLI on `partially_supported` precision. |
| 10 | **Evidence graph** (independence score, retraction cascade), **adversarial branch**, **calibrated confidence**, mid-run budget re-estimation. | Reports show per-claim independence and counter-view sections; stated coverage matches empirical coverage within 5 points on held-out pairs. |
| 11 | **Learned PRM prior** (`calibrate` subcommand), cross-session evidence memory, **living reports** (`watch`). | Prior improves consensus-set prediction over the 0.0 baseline on held-out sessions; `watch` emits a diff when a fixture source is retracted. |
| 12 | **Persian / cross-lingual** (`fa` feature), **WASM adapters** (`wasm-adapters`), public SciFact scoreboard in README. | A Persian query retrieves and verifies against English evidence; a third-party WASM adapter runs with no filesystem access; `bench verify` numbers published. |

Rough effort: Phases 1-4 about two to three focused weeks; Phase 5 another
two, most of it in evaluation data rather than code. Phases 7-12 are each one
to three weeks and can be reordered freely after Phase 7, which the rest
depend on for test fixtures.

## 9. Risks and how to hold them

| Risk | Mitigation |
|---|---|
| `rmcp` 3.x API churn (3.3 -> 3.5.1 in four weeks). | Pin `=3.5.1`; wrap SDK types in `re-mcp` only; Dependabot PRs run the parity harness. |
| `ort` is `2.0.0-rc.14`, no stable release; native ONNX Runtime download at build time. | `semantic` feature off by default; vendor the runtime via `ort`'s `download-binaries` only in release builds; document CPU-only support first. |
| Model downloads (90-280 MB) are a surprise for a "no API key" project. | Lazy download on first `nli` use, clear log line, `OFFLINE=1` refuses, `lexical` always works. |
| OpenAlex credit budgets could throttle keyless users. | Read `X-RateLimit-Credits-Used`, surface in `get_scholar_auth_status`, recommend the free key in README. |
| Behaviour drift during "improvements". | Legacy algorithms kept behind flags; golden vectors never regenerated silently. |
| A prompt in `setup` corrupts an MCP stdio stream. | `setup` runs only on an interactive TTY and never from the `mcp` subcommand; all answers have flag and env equivalents. |
| Users assume a port is open because the Python version had `serve`. | HTTP is off until `setup` or `--http`; `serve` prints the hint and exits non-zero. |
| Phases 7-12 balloon the binary and build time. | Each lives behind its own Cargo feature; the default build stays stdio + lexical + bundled SQLite. |

## 10. Decisions (defaults an agent proceeds with unless Raouf overrides)

| # | Question | Default the executable plans assume |
|---|---|---|
| 1 | Default verifier tier for `standard` mode | `lexical` until Phase 5 lands, then `hybrid`; `high_stakes` always `nli` when the `semantic` feature is built. |
| 2 | Tunnel launcher: shell or CLI | Stays shell (`run-notion-mcp-laptop.sh`); `setup` only offers to run it. |
| 3 | Scholar Gateway adapter | Kept, behind the `scholar-gateway` Cargo feature, off by default. |
| 4 | `setup` offers model download | Yes, default answer No. |
| 5 | First post-cut-over phase | Phase 7 (replayable runs). |

An agent never blocks on these. It records the default it used in the commit
message of the task that first depends on it.

## 11. Parity hazards (read before porting a single function)

Verified against the Python code and `golden_vectors.json` on 2026-10-18.

| Hazard | Python behaviour | Rust requirement |
|---|---|---|
| Dict order | `select_branches` and `select_best_ucb` iterate dicts in insertion order; sorting is stable; ties keep insertion order. | Parse `scores`/`branches` with `serde_json` feature `preserve_order` into `IndexMap`; use `sort_by` (stable), never `sort_unstable_by`. |
| Tokenisation | `query.lower().split()` keeps punctuation attached: `"implications?"` does not match the keyword `implications` (difficulty 0.0479 vs 0.0542, verified). | `to_lowercase().split_whitespace()`, no punctuation stripping. |
| Multi-word keywords | Matched by substring on the full lowercased query (`"beam search" in query_lower`). | Same substring check; do not tokenise. |
| Float arithmetic | `0.25*a + 0.35*b + 0.25*c + 0.15*d` evaluated left to right in f64. | Identical expression order; no FMA; `f64` only. |
| JSON float parsing | `json.loads` is correctly rounded. | Enable `serde_json` feature `float_roundtrip`. Without it serde_json read `0.24062499999999998` as `0.240625` (found in Phase 1 Task 3). |
| `round(x, 4)` | Python rounds the exact binary value, ties to even. | Implement `py_round` and test it against every `relevance_score` golden vector; add tie cases to the generator if any disagree. |
| `float("inf")` | UCB returns infinity for unvisited nodes; `max(total_visits, 1)` guards `log(0)`. | `f64::INFINITY`; the golden file stores the infinity case as a boolean because JSON has no infinity. |
| HTML entities | `html.unescape` decodes named, decimal and hex references with the HTML5 table, legacy names without `;` (`&copy`), `&#0;` to U+FFFD, and the Windows-1252 remap (`&#x80;` to `€`). | `html-escape` gets all three wrong (found in Phase 1 Task 6). Use `re_core::html_unescape`, an exact port of the CPython algorithm with tables generated from CPython by `scripts/gen_html_entities.py`. |
| Regex semantics | Python `re` on `str`: `\b`, `\s`, `\w` are Unicode-aware; `re.DOTALL` on script/style patterns. | `regex` crate defaults are Unicode-aware; use `(?s)` for the script/style patterns; `[​-‏]` becomes `[\x{200b}-\x{200f}]`. |
| Sanitize twice | Not idempotent (section 5). | Call once per field; keep the behaviour. |
| Set iteration | `plan_research_angles` iterates `set(words)`, so with two or more domain hints the angle order depends on `PYTHONHASHSEED` (22 orders across 64 seeds for one query; found in Phase 1 Task 7). | First-occurrence order. Golden expected values come from Python's own algorithm with `set` swapped for an insertion-ordered dict; the generator asserts this is a no-op wherever Python is stable. Fixing the Python side to match is a one-line change worth making before cut-over. |
| Claim IDs | `claim_0001` zero-padded, sentence split on `(?<=[.!?])\s+` (lookbehind). | `regex` has no lookbehind: split on `[.!?]\s+` while keeping the terminator, then `trim_end_matches(['.', '!', '?'])`. |
| Timestamps and UUIDs | `utc_now()` is second-precision ISO-8601 with a `Z` suffix (`2026-10-18T22:34:00Z`); IDs are `uuid4`. | `re_model::clock::utc_now` formats identically; parity harness ignores these fields by name. |
| JSON text | Everything hashed or stored (`stable_json_hash`, DB payloads, run-pack files) uses Python's default `ensure_ascii=True`, so non-ASCII becomes `\uXXXX` with surrogate pairs. Floats use `float.__repr__` (`1e-05`, `1e+16`). Only MCP tool responses use `ensure_ascii=False`. | `serde_json` writes `1e-5`, `1e16` and raw UTF-8, so it cannot be used for these bytes. Use `re_core::pyjson::dumps`, checked byte-for-byte against Python in four modes (found in Phase 2). |
| `str.isalnum` (dedup keys) | Letters (`L*`) and numbers (`N*`) only. | `char::is_alphanumeric` also keeps `Other_Alphabetic` combining marks (U+093F is kept by Rust, dropped by Python), splitting dedup keys. Use `[^\p{L}\p{N}]+` removal (found in Phase 3). |
| `str.strip` / `str.split` | Python whitespace adds `\x1c`..`\x1f` to Unicode `White_Space`. | `re_retrieval::base::py_strip` / `py_is_space`, used by snippets, years, DOIs and queries. |
| ElementTree `findtext` | `.text` of the first direct child: text before the first sub-element, entities expanded, `None` when absent. Nested matches are ignored. | The arXiv parser tracks depth and stops collecting at the first child start tag. Unit-tested on nested, repeated and empty children. |
| XML hardening | `defusedxml` rejects entity declarations; expat rejects undefined entities, bad characters, junk after the root, unbound prefixes. | Expand only the five predefined entities and legal character references; reject any `<!ENTITY`; 4 MiB body cap. Error messages differ, types match. |
| Dict overwrite order | `errors[name] = err` with a repeated name keeps the first position and the last value. | `IndexMap::insert` has the same semantics; the `all_fail` aggregate vector covers it. |
| DBLP from datacenter IPs | HTTP 202 with an anti-bot HTML page; Python's `urllib` treats 202 as success and the JSON parse fails. | Any 2xx is success; the page surfaces as `malformed_response` (`dblp__bot_challenge` fixture). Recording falls back to a hand-built `dblp__realistic` body. |
| Tool arguments | FastMCP `json.loads` a string argument for any non-`str` parameter and substitutes lists, objects and null; pydantic then validates in lax mode (`true` and `"3.0"` are ints, `"nan"` is a float) and reports every failing field and union member. | `re_mcp::args::check` runs the same pass before any tool body and formats errors like pydantic, minus its version-specific `[type=...]` and URL suffixes (found in Phase 4). |
| Tool errors | FastMCP returns failures as results with `isError: true` and `Error executing tool <name>: <message>`, not JSON-RPC errors. | Same shape and text; the model can read and correct them. |
| Python whitespace | `\s`, `strip()` and `split()` include `\x1c`..`\x1f`; Rust's `\s`, `trim()` and `split_whitespace()` do not. | `re_core::pystr`; used by claims, difficulty, validation, memory recall and the adapters. The sanitizer was already safe because control characters become spaces before injection matching. |
| `json.JSONDecodeError` | Messages like `Expecting value` with a character `pos` (the opening quote for an unterminated string, the input length at EOF). | `re_mcp::validate` maps serde errors to Python's wording and position for the common cases. |
| Branch listing order | `get_session_state` orders by `(created_at, id)`; branches created in the same second tie-break on random UUIDs, so Python's own order is undefined. | Same SQL; the parity harness compares that list unordered. |

## 12. Gauntlet log (2026-10-18)

Line-by-line review of this plan. Each row was fixed in place.

| Found | Fix |
|---|---|
| TL;DR said 6 crates after `re-ui` made it 7. | 7. |
| Speed numbers read as promises. | Marked as hypotheses to be measured by the Phase 1 bench task. |
| `reqwest` feature named `rustls-tls`; crates.io lists `rustls`. | Corrected, full feature list given. |
| Paper authors were guessed ("Wang, X", "Yu", "Zhou"). | Pulled from arXiv: Wang, Xinglin et al.; Chen, Zhengyu et al.; Song, Zeen et al.; Zheng, Congmin et al. |
| CATS labelled ICLR 2026 on the strength of one secondary site. | arXiv page lists no venue; cited as a preprint with the discrepancy noted. |
| Tool example used an invented `ToolError` and `Text::bounded`. | `McpError` (real) and `BoundedText` (ours, labelled as ours); macro syntax verified against rmcp 3.5 docs. |
| Phase 2 promised "opens unchanged" while 3.1 bumped `user_version`. | Additive migrations only; `user_version` advisory because Python rewrites it. |
| `setup` wording implied any terminal run could prompt. | Only `setup` prompts; others print a hint. |
| "reqwest compiled client-only" is meaningless. | Axum/hyper server stack excluded without `local-host`. |
| Property test claimed `sanitize` idempotent and "monotone in word count". | Both false; verified counter-example; replaced with true properties. |
| Dedup cited "arXiv ID to DOI mapping both expose". | DataCite `10.48550/arXiv.<id>` plus Atom `arxiv:doi`. |
| Progressive widening rule `visits > depth^2` was invented. | Standard `children < ceil(C * visits^alpha)`. |
| Docker image would silently lose `serve` once `local-host` is opt-in. | Phase 6 builds the image with the feature and starts with `--http`. |
| Open decisions would block an autonomous agent. | Converted to defaults with an override path. |
| *Phase 1 execution:* serde_json misparsed a golden float by one ULP. | `float_roundtrip` feature; hazard row added. |
| *Phase 1 execution:* `html-escape` is not `html.unescape`. | Exact CPython port with generated tables; crate dropped. |
| *Phase 1 execution:* "sanitize output matches no injection pattern" is false; nested payloads bypass the live Python sanitizer. | Property corrected; fixed in both implementations (section 7). |
| *Phase 1 execution:* Python angle planning is hash-seed dependent. | Deterministic port plus hazard row. |
| *Phase 1 execution:* Task 7 listed `auto` as a research mode; it is a profile value. | Golden uses real modes plus error vectors. |
| *Phase 2 execution:* MSRV 1.88 was wrong; `uuid` 1.28 needs 1.89, `ort` 1.92, `wasmtime` 1.97. | Default-build MSRV 1.89, optional floors documented, CI checks 1.89. |
| *Phase 2 execution:* the plan put record types in `re-verify`, creating a `re-store`/`re-verify` cycle. | New `re-model` crate. |
| No list of Python-vs-Rust parity traps. | Section 11 added, with verified examples. |
| *Phase 3 execution:* plan said 8 s HTTP timeout; Python uses 25 s. | Kept 25 s for parity (`REASONING_ENGINE_HTTP_TIMEOUT_SECS` overrides); fan-out deadlines belong in Phase 5 tuning, measured. |
| *Phase 3:* error messages cannot match (different JSON and XML parsers). | Parity compares `error_type`, `retryable` and `metadata`; messages are excluded by name. 429 metadata gains `retry_after_secs` (additive). |
| *Phase 3:* malformed item shapes (a string where Python expects a dict) crash Python adapters. | Rust treats them as missing fields; DBLP list-valued `ee`/`venue` take the first element. Documented divergence, never reachable on well-formed provider JSON. |
| *Phase 3:* Semantic Scholar answered 429 while recording. | Recorder skips rate-limited sources and retries later; no keys or contact email are committed (`reasoning-engine@example.org`). |
| *Phase 3:* plan asked for per-source `*.expected.json`. | One `tests/fixtures/http/expected.json` (20 cases, 21 URLs, 3 aggregator vectors) from `record_http.py expected`; the parity test was mutation-checked. |
| *Phase 3:* `webpki-root-certs` (Mozilla CA data via reqwest) is CDLA-Permissive-2.0. | `deny.toml` exception scoped to that crate only. |
| *Phase 4 execution:* plan said validation errors become `McpError` codes `-32000..-32019`. | Python returns `isError` tool results; the port matches it, which the current spec also prefers. |
| *Phase 4:* plan said bool-as-int is rejected. | Pydantic accepts `true` as 1 before `_coerce_int` runs; the port matches Python. |
| *Phase 4:* plan compared Rust `structuredContent` with Python text. | The harness compares text, which is byte-identical to `json.dumps(payload, ensure_ascii=False)`; `structuredContent` carries the same payload (arrays as `{"result": [...]}`). Result: 46 scripted steps identical after normalisation. |
| *Phase 4:* Python run packs written on Windows use `\r\n`, so their hashes differ by platform. | Rust always writes `\n`. Verification also re-checks `run_pack_hash` and rejects artifact names that are not plain file names, so a rewritten attestation no longer reads valid. |
| *Phase 4:* an empty `REASONING_ENGINE_DB` made the Rust server open path `""`. | Empty values count as unset, as in Python's `db_path or DEFAULT`. Defaults move to `~/.reasoning-engine/{reasoning.db,runs}`. |

## References (Harvard, Australian style)

Anush008 2026, *FastEmbed-rs: local vector embeddings and reranking in Rust*, version 7.1.1, crates.io, viewed 18 October 2026, <https://crates.io/crates/fastembed>.

Model Context Protocol 2026a, *Key changes: specification revision 2026-07-28*, viewed 18 October 2026, <https://modelcontextprotocol.io/specification/2026-07-28/changelog>.

Model Context Protocol 2026b, *Streamable HTTP transport*, viewed 18 October 2026, <https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/streamable-http>.

Model Context Protocol 2026c, *rust-sdk: the official Rust SDK for the Model Context Protocol* (rmcp 3.5.1), GitHub, viewed 18 October 2026, <https://github.com/modelcontextprotocol/rust-sdk>.

Model Context Protocol 2026d, *rmcp: handler::server::router::tool*, docs.rs, viewed 18 October 2026, <https://docs.rs/rmcp/latest/rmcp/handler/server/router/tool/index.html>.

OpenAlex 2026, *Rate limits and authentication*, updated 15 October 2026, viewed 18 October 2026, <https://help.openalex.org/how-to-use-the-api/rate-limits-and-authentication>.

Reimers, N & Gurevych, I 2026, *Pretrained cross-encoder models: NLI*, Sentence-Transformers documentation, viewed 18 October 2026, <https://sbert.net/docs/cross_encoder/pretrained_models.html>.

Rust Project 2026, *Rust release schedule* (stable 1.99.1), releases.rs, viewed 18 October 2026, <https://releases.rs/>.

Thoughtworks 2026, *Technology Radar: cargo-mutants*, April 2026, viewed 18 October 2026, <https://www.thoughtworks.com/radar/tools/cargo-mutants>.

Chen, Z, Wang, Y, Xiao, T, Zhou, R, Yang, X, Wang, W, Sui, Z & Wang, J 2025, 'From mathematical reasoning to code: generalization of process reward models in test-time scaling', *arXiv preprint*, arXiv:2506.00027, viewed 18 October 2026, <https://arxiv.org/abs/2506.00027>.

Song, Z, Qiang, W, Zhao, S, Zheng, C & Hua, G 2025, 'Reward model generalization for compute-aware test-time reasoning', *arXiv preprint*, arXiv:2505.18065, viewed 18 October 2026, <https://arxiv.org/abs/2505.18065>.

Wang, X, Li, Y, Feng, S, Yuan, P, Zhang, Y, Shi, J, Tan, C, Pan, B, Hu, Y & Li, K 2025, 'Every rollout counts: optimal resource allocation for efficient test-time scaling', *arXiv preprint*, arXiv:2506.15707 (v2, 20 October 2025), viewed 18 October 2026, <https://arxiv.org/abs/2506.15707>.

Zheng, C, Zhu, J, Ou, Z et al. 2025, 'A survey of process reward models: from outcome signals to process supervisions for large language models', *arXiv preprint*, arXiv:2510.08049, viewed 18 October 2026, <https://arxiv.org/abs/2510.08049>.
