# Rust Rewrite Plan (October 2026)

Status: proposal, researched against docs current as of 2026-10-18.
Scope: port the `reasoning-engine` MCP server from Python to Rust, keep the
`/deep-research` skill and every tool name unchanged, and use the port to fix
the accuracy ceilings the Python MVP documents about itself.

## 1. TL;DR

- **Do it as a 6-crate Cargo workspace** on Rust 1.99 / edition 2024, using the
  official `rmcp` 3.5.1 SDK (which implements MCP `2026-07-28`), `tokio`,
  `reqwest`, `rusqlite` (bundled, WAL), `schemars` for tool schemas.
- **Speed wins are real but specific.** Expect ~10x faster cold start, ~5-10x
  lower RSS, and much better fan-out behaviour (per-source deadlines, connection
  reuse, backpressure). The deterministic maths is already fast in Python; the
  slow parts are process start, blocking `urllib` fan-out with a 25 s timeout,
  and opening a fresh SQLite connection per call.
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
| `reqwest` | 0.13.5 | 2026-09-08 | HTTP client with pooling, HTTP/2, per-request timeouts. Use `rustls`. |
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
- PRM survey (Zheng et al. 2025) and Yu et al. (2025) on compute scaling:
  Best-of-N is the right default under tight budgets, MCTS/beam only when
  compute is plentiful. Our difficulty tiers already encode that; the data to
  calibrate the thresholds is sitting unused in `episodic_memory`.
- CATS (ICLR 2026) allocates per-step compute using a learned controller. Out
  of scope for a deterministic server, but it argues for logging per-step
  score variance so a future controller has training data.

## 3. Target architecture

```
reasoning-engine/            (Cargo workspace, edition 2024, MSRV 1.88 for rmcp)
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
| `db.py`, `sessions.py`, `memory.py` | `re_store::{schema, sessions, memory}` | One pooled connection per thread, WAL, `PRAGMA user_version = 3`. |
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
    ) -> Result<Json<SessionCreated>, ToolError> {
        let query = Text::bounded(p.query, limits::MAX_QUERY_CHARS)?;
        Ok(Json(self.sessions.create(query).await?))
    }
}
```

Each tool lives in its own file and is registered with
`ToolRouter::new().with_async_tool::<T>()` in a fixed order, as the rmcp docs
recommend once a server grows past a handful of tools.

### 3.3 Optional local host: HTTP server, audit UI, tunnel

Nothing listens on a port unless the user asks. The first time the binary
runs in a terminal (or whenever `reasoning-engine setup` is invoked) it asks:

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
  subcommands at all, and `reqwest` is compiled client-only.
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
   monotone in word count for fixed keywords; DORA's three output lists are
   pairwise disjoint and together cover every input branch; `budget_remaining`
   never goes negative; `sanitize` is idempotent and never grows the input
   beyond entity expansion; `normalize_doi` is idempotent.
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
- **Dedup:** DOI first, then arXiv ID to DOI mapping (arXiv and Crossref both
  expose it), then fuzzy title match (normalised Levenshtein <= 0.08) so
  preprint and published versions merge instead of double counting.
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
   progressive widening: allow a new child only when `visits > depth^2`.
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
| 6 | Cut-over: README, Dockerfile (distroless, `FROM scratch` + CA certs), `.mcp.json` examples; `re-ui` audit UI under `local-host`; Python marked deprecated for one release, removed next. | Users swap the `command` line only. UI reachable only when `setup` enabled it. |
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

## 10. Open decisions for Raouf

1. Default verifier tier for `standard` mode: `lexical` (zero download) or
   `hybrid` (130 MB download, much better ranking)? Recommendation: `hybrid`
   once Phase 5 lands, `lexical` until then.
2. Keep the Notion/Cloudflare tunnel launcher scripts as shell, or fold them
   into `reasoning-engine serve --tunnel`? Recommendation: keep shell; it is
   platform glue, not engine.
3. Drop Scholar Gateway adapter? It is the only paid source and the mock
   fixtures add surface area. Recommendation: keep behind a feature flag.
4. Should `setup` also offer to download the semantic models up front, or
   leave that to first use? Recommendation: offer it, default No.
5. Which Phase 7-12 item gets first dibs after cut-over? Recommendation:
   Phase 7 (replayable runs) because everything else tests against it.

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

Wang, X et al. 2025, 'Every rollout counts: optimal resource allocation for efficient test-time scaling', *arXiv preprint*, arXiv:2506.15707, viewed 18 October 2026, <https://arxiv.org/abs/2506.15707>.

Yu, Z et al. 2025, 'From mathematical reasoning to code: generalization of process reward models in test-time scaling', *arXiv preprint*, arXiv:2506.00027, viewed 18 October 2026, <https://arxiv.org/abs/2506.00027>.

Zheng, C et al. 2025, 'A survey of process reward models: from outcome signals to process supervisions for large language models', *arXiv preprint*, arXiv:2510.08049, viewed 18 October 2026, <https://arxiv.org/abs/2510.08049>.

Zhou, Y et al. 2026, 'Reward model generalization for compute-aware test-time reasoning (CATS)', *International Conference on Learning Representations 2026*, arXiv:2505.18065, viewed 18 October 2026, <https://arxiv.org/abs/2505.18065>.
