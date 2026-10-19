# Rust Port Implementation Plan: Phases 0-6

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. Read `docs/rust-rewrite-plan-2026.md` sections 10 (decision defaults) and 11 (parity hazards) before Task 1.

**Goal:** Replace the Python MCP server with a Rust binary that exposes the same 20 tools with identical names, argument names and JSON shapes, proves parity against golden vectors and a live parity harness, and keeps `skill/deep-research.md` unchanged.

**Architecture:** Cargo workspace with `re-core` (pure algorithms), `re-store` (SQLite), `re-retrieval` (HTTP adapters + aggregator), `re-verify` (claims, gate, run packs), `re-mcp` (rmcp server), `re-ui` (feature `local-host`), `re-cli` (binary). Python stays in `src/` untouched until Task 24.

**Tech Stack:** Rust stable (edition 2024, `rust-version = "1.88"`), rmcp 3.5.1, tokio 1.53, reqwest 0.13 (`rustls`), rusqlite 0.40 (`bundled`), serde/serde_json (`preserve_order`), schemars 1.2, indexmap, regex, proptest, insta, criterion, cargo-nextest, cargo-deny, wiremock. Python 3.12+ with `.venv` for golden generation and the parity harness.

---

## Agent contract

- **Branching:** one branch per phase, named `rust/phase-<n>-<slug>` from `master`. One PR per phase. Commits use `type(scope): subject` (`feat(core): ...`, `test(store): ...`, `chore(ci): ...`).
- **Verification before every commit:** `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo nextest run --workspace`, and the Python suite `pytest -q` (it must stay green; the Python code is the oracle until cut-over). From Task 7 also `cargo deny check`.
- **Golden vectors:** never edit `tests/golden/golden_vectors.json` by hand. Regenerate only via `python3 -I tests/golden/generate_golden.py`, only when the task says to extend the generator, and commit the generator change and the JSON in the same commit.
- **Python is read-only** until Task 24, except `tests/golden/generate_golden.py` and new fixture scripts under `tests/fixtures/`.
- **No secrets, no model weights, no `runs/` output** in git. `.gitignore` gets `target/`, `models/`, `*.onnx`.
- **Decision defaults** come from `rust-rewrite-plan-2026.md` section 10. Do not ask; record the default used in the commit body.
- **Stop and report** only if: a golden vector cannot be matched after implementing the listed parity hazard fix, a crate pinned in the plan fails to build on the current stable, or the parity harness shows a difference that is not an ID, timestamp or float formatting artefact.
- **Environment prerequisites:** `cargo >= 1.88` (`rustup update stable`), `cargo install cargo-nextest cargo-deny cargo-insta`, `python3 -m venv .venv && . .venv/bin/activate && pip install -e ".[dev]"`.

---

## Phase 0: Golden vectors (done)

`tests/golden/generate_golden.py` and `golden_vectors.json` exist on `docs/rust-rewrite-plan-2026`. Task 6 extends them.

## Phase 1: Workspace and `re-core`

### Task 1: Workspace skeleton and CI

**Files:**
- Create: `Cargo.toml` (workspace), `rust-toolchain.toml`, `deny.toml`, `.cargo/config.toml`, `clippy.toml`, `.config/nextest.toml`
- Create: `crates/re-core/Cargo.toml`, `crates/re-core/src/lib.rs`
- Create: `.github/workflows/rust.yml`
- Modify: `.gitignore`, `.dockerignore`

Steps:

- [ ] Write the workspace `Cargo.toml` with `[workspace] members = ["crates/*"]`, `resolver = "3"`, `[workspace.package] edition = "2024", rust-version = "1.88", license = "MIT"`, and `[workspace.dependencies]` pinning every crate from the plan's section 2.1 table with `=` versions.
- [ ] Add `[workspace.lints.rust] unsafe_code = "forbid"` and `[workspace.lints.clippy] pedantic = "warn"`; each crate sets `[lints] workspace = true`.
- [ ] `rust-toolchain.toml`: `channel = "stable"`, components `rustfmt`, `clippy`.
- [ ] `deny.toml`: advisories deny unmaintained and yanked; licenses allow MIT, Apache-2.0, BSD-2/3, ISC, Unicode-3.0, MPL-2.0; bans `multiple-versions = "warn"`.
- [ ] `re-core/src/lib.rs` with `#![forbid(unsafe_code)]`, `#![deny(missing_docs)]`, empty modules `difficulty`, `dora`, `ucb`, `sanitize`, `planning`, `claims`, `gate`, `profiles`, `types`.
- [ ] `.github/workflows/rust.yml`: matrix ubuntu/macos/windows, `dtolnay/rust-toolchain@stable`, `Swatinem/rust-cache`, steps fmt check, clippy `-D warnings`, `cargo nextest run --workspace --all-features`, `cargo deny check` (ubuntu only), `cargo doc --no-deps` with `RUSTDOCFLAGS=-D warnings`.
- [ ] Run `cargo build --workspace` and `cargo nextest run --workspace` (zero tests is fine).
- [ ] Commit `chore(rust): add cargo workspace, lints and CI`.

### Task 2: `re-core::types` and golden loader

**Files:**
- Create: `crates/re-core/src/types.rs`, `crates/re-core/tests/golden.rs`, `crates/re-core/tests/common/mod.rs`

Steps:

- [ ] Add `Score(f64)` newtype with `Score::new(f64) -> Result<Score, CoreError>` rejecting NaN and values outside `[0, 1]`, `Deserialize` via `try_from`, `Serialize` as plain f64.
- [ ] Add `Strategy` enum (`SinglePass`, `BestOfN`, `BeamSearch`, `Forest`) serialising to the Python strings `single_pass`, `best_of_n`, `beam_search`, `forest`; `BranchStatus` enum (`active`, `reflecting`, `pruned`, `completed`); `Allocation` enum (`breadth`, `depth`, `budget_exhausted`).
- [ ] Add `CoreError` with `thiserror`.
- [ ] `tests/common/mod.rs`: load `../../../tests/golden/golden_vectors.json` via `include_str!` into a `serde_json::Value` (feature `preserve_order` on).
- [ ] Test that the golden file parses and has the nine top-level keys.
- [ ] Commit `feat(core): add score newtypes, enums and golden loader`.

### Task 3: `re-core::difficulty`

**Files:**
- Create: `crates/re-core/src/difficulty.rs`
- Modify: `crates/re-core/tests/golden.rs`

Steps:

- [ ] Write the golden test first: for each entry in `difficulty`, assert `estimate_difficulty(query) == expected` with exact `f64` equality.
- [ ] Port `COMPLEX_KEYWORDS`, `DOMAIN_KEYWORDS` (split into single-word set and multi-word list at compile time), `MULTI_PART_SIGNALS` regex (case-insensitive, Unicode `\b`).
- [ ] Tokenise with `to_lowercase().split_whitespace()`; do not strip punctuation (hazard table).
- [ ] Compute the four component scores and combine in the exact Python order `0.25*len + 0.35*kw + 0.25*struct + 0.15*q`, then clamp.
- [ ] Add a `proptest` that the result is always in `[0, 1]` and non-decreasing when appending words drawn from an alphabet of non-keyword, non-conjunction tokens.
- [ ] Run `cargo nextest run -p re-core`.
- [ ] Commit `feat(core): port difficulty estimator with golden parity`.

### Task 4: `re-core::dora`

**Files:**
- Create: `crates/re-core/src/dora.rs`
- Modify: `crates/re-core/tests/golden.rs`

Steps:

- [ ] Golden tests first for `allocate_budget` (10 vectors) and `select_branches` (30 vectors): deserialize `scores` into `IndexMap<String, f64>` and compare the full `AllocationResult` including list order.
- [ ] Port `BudgetAllocation` and `AllocationResult` as serde structs with the same field names.
- [ ] Port `select_branches` using a stable `sort_by` on `(score desc)`; `kappa = max - min` when more than one score; the budget-trim step must preserve Python's `(to_continue + to_reflect)[:budget]` order.
- [ ] Property tests: outputs pairwise disjoint; union equals input keys; `budget_remaining >= 0`; `budget_remaining == 0` or `allocation == budget_exhausted` when input budget is 0.
- [ ] Commit `feat(core): port DORA budget allocation and branch selection`.

### Task 5: `re-core::ucb`

**Files:**
- Create: `crates/re-core/src/ucb.rs`
- Modify: `crates/re-core/tests/golden.rs`

Steps:

- [ ] Golden tests for `ucb_score` (exact equality), `ucb_unvisited_is_infinite`, and `select_best_ucb` (3 vectors, `IndexMap` input, strict `>` so the first maximum wins).
- [ ] Port with `DEFAULT_C = 1.4`, `max(total_visits, 1)`.
- [ ] Commit `feat(core): port UCB1 selection`.

### Task 6: `re-core::sanitize`

**Files:**
- Create: `crates/re-core/src/sanitize.rs`
- Modify: `crates/re-core/tests/golden.rs`

Steps:

- [ ] Golden tests for the 8 sanitizer vectors.
- [x] Decode entities with `re_core::html_unescape::unescape` first (an exact port of CPython's `html.unescape`; `html-escape` was tried and differs on `&copy`, `&#0;` and `&#x80;`), then apply the regexes in the exact Python order: zero-width, control chars to space, `(?is)<script...>`, `(?is)<style...>`, markdown links to `text (url)`, tags, each injection pattern, whitespace collapse, trim.
- [ ] Add a `MAX_SANITIZE_INPUT` guard (1 MiB) that truncates before regex execution and document it (new behaviour, not in Python, allowed because inputs are already bounded at the MCP layer).
- [x] Property tests: output has no `<...>` tag; a single injection phrase is removed; output never has more characters than input. (Nested payloads survive in Python too; see plan section 7.)
- [ ] Add a unit test documenting non-idempotence with the verified example.
- [ ] Commit `feat(core): port content sanitizer`.

### Task 7: Extend golden vectors, port planning, profiles, claims, gate

**Files:**
- Modify: `tests/golden/generate_golden.py`
- Create: `crates/re-core/src/{planning,profiles,claims,gate}.rs`
- Modify: `crates/re-core/tests/golden.rs`

Steps:

- [ ] Extend the generator with sections `plan_research_angles` (6 queries x `max_angles` 1, 3, 6), `evidence_gap_questions` (3 claim lists), `classify_research_mode` (10 queries x modes `standard`, `quick`, `high_stakes`) and `select_profile` (10 queries x `auto`, `law`), error vectors for unknown modes and profiles, `claim_type` (12 sentences), `extract_claims_text` (2 drafts, only `text`, `claim_type`, `importance`, `risk_level`, `requires_citation`), `quality_gate` (4 claim/verification combinations, `checked_at` removed). Regenerate and commit generator plus JSON together: `test(golden): extend vectors for planning, profiles, claims and gate`.
- [ ] Port `research.py` (`ANGLE_LIBRARY`, `DOMAIN_HINTS`, `QUESTION_STARTERS`, `unique_ordered`) to `planning.rs`; golden tests.
- [ ] Port `profiles.py` (all ten `ResearchProfile`s, `PROFILE_KEYWORDS`, `select_profile`, `classify_research_mode`) to `profiles.rs`; golden tests.
- [ ] Port `_claim_type` and the sentence split (hazard table: no lookbehind) to `claims.rs`; golden tests.
- [ ] Port `run_quality_gate` to `gate.rs` with `BLOCKING_STATUSES`; golden tests.
- [ ] Commit `feat(core): port planning, profiles, claim typing and quality gate`.

### Task 8: Benchmarks and supply-chain gate

**Files:**
- Create: `crates/re-core/benches/core.rs`, `scripts/bench-startup.sh`
- Modify: `.github/workflows/rust.yml`

Steps:

- [ ] `criterion` benches for `estimate_difficulty` (long query), `select_branches` (8 branches), `sanitize` (50 KiB HTML).
- [ ] `scripts/bench-startup.sh`: `hyperfine --warmup 3 '.venv/bin/reasoning-engine --help' 'target/release/reasoning-engine --help'` (the Rust binary arrives in Task 18; until then the script exits 0 with a notice).
- [ ] Add `cargo deny check` to CI and run it locally.
- [ ] Commit `chore(bench): add criterion benches and startup benchmark script`.
- [ ] Open PR `rust/phase-1-core` with the golden parity summary in the body.

## Phase 2: `re-store`

### Task 9: Schema, connection, additive migration

**Files:**
- Create: `crates/re-store/Cargo.toml`, `crates/re-store/src/{lib,schema,conn,error}.rs`
- Create: `tests/fixtures/make_python_db.py`
- Create: `crates/re-store/tests/compat.rs`

Steps:

- [ ] `tests/fixtures/make_python_db.py`: using the Python package, create a DB with one session, three branches, one reflection, one memory, one research run with evidence, claims, verifications, gate and provenance. Write to a path given on argv. Commit the script, not the DB.
- [ ] `conn.rs`: open with `rusqlite` `bundled`, apply `foreign_keys = ON`, `busy_timeout = 5000`, `journal_mode = WAL`, `synchronous = NORMAL`, `trusted_schema = OFF`; create parent dirs; `chmod 0o600` on first creation (unix).
- [ ] `schema.rs`: the 13 `CREATE TABLE IF NOT EXISTS` statements copied verbatim from `db.py`, plus indexes. Do not set `user_version`.
- [ ] `compat.rs`: run the Python fixture script in a temp dir, open with `re-store`, read back every row count and the session's `budget_remaining_steps`.
- [ ] Commit `feat(store): add SQLite schema, hardened connection and Python-DB compatibility test`.

### Task 10: Sessions, branches, reflections, memory

**Files:**
- Create: `crates/re-store/src/{sessions,branches,memory}.rs`, `crates/re-store/tests/sessions.rs`

Steps:

- [ ] Port `create_session`, `get_session`, `register_branch` (depth check, parent ownership, sources insert), `score_branch` (ownership check, `visits + 1`, `total_changes == 0` error), `update_branch_status`, `apply_planning_result` (single transaction), `get_active_branches`, `get_session_branches` (`ORDER BY created_at, id`), `check_termination` (thresholds 0.85 / 0.80), `get_consensus_candidates`.
- [ ] Port `save_memory`, `recall_memory` (word and tag overlap, tags split on `-`, sorted by relevance desc, stable), `record_reflection`.
- [ ] Re-express every assertion in `tests/test_sessions.py`, `tests/test_memory.py`, `tests/test_db.py` as Rust tests (same inputs, same expected errors).
- [ ] Commit `feat(store): port session, branch, reflection and memory lifecycle`.

### Task 11: Verifiable store

**Files:**
- Create: `crates/re-store/src/research.rs`, `crates/re-store/tests/research.rs`
- Create: `crates/re-verify/src/models.rs` (moved here early because the store needs the record types)

Steps:

- [ ] Port `models.py` record types (`ResearchRun`, `EvidenceRecord`, `EvidenceGap`, `ClaimRecord`, `VerificationRecord`, `utc_now`, `stable_json_hash` with sorted keys and `ensure_ascii=False` semantics) to `re-verify/src/models.rs`.
- [ ] Port `store.py`: `save_run`, `save_evidence` (snippet hash), `save_gap`, `save_claim`, `save_verification`, `save_gate`, `append_provenance`, `list_*` ordered by `rowid`.
- [ ] Re-express `tests/test_verifiable_store.py` and `tests/test_verifiable_models.py`.
- [ ] Cross-check `stable_json_hash` against three hashes produced by the Python function on the same inputs (add them to the generator in this task).
- [ ] Commit `feat(store): port verifiable research ledger`.
- [ ] Open PR `rust/phase-2-store`.

## Phase 3: `re-retrieval`

### Task 12: Adapter base and HTTP recording harness

**Files:**
- Create: `crates/re-retrieval/Cargo.toml`, `crates/re-retrieval/src/{lib,base,error,client}.rs`
- Create: `tests/fixtures/http/README.md`, `tests/fixtures/record_http.py`

Steps:

- [ ] `client.rs`: one `reqwest::Client` with `rustls`, `gzip`, `http2`, 8 s per-request timeout (configurable), pool idle 90 s, `User-Agent` `reasoning-engine/<version> (mailto:<REASONING_ENGINE_CONTACT_EMAIL>)`.
- [ ] `base.rs`: `relevance_score` (with `py_round`, golden), `normalize_doi` (golden), `coerce_year`, `clip_snippet`, `strip_tags`, `build_record`; `AdapterError` mapping 401 to `auth_required`, 429 to `rate_limited` (retryable, honour `Retry-After`), others `unavailable`; JSON decode errors to `malformed_response`.
- [ ] `record_http.py`: for each of the seven sources, call the Python adapter's URL builder for the query `"process reward model"` with `limit=5`, save the raw body and status to `tests/fixtures/http/<source>.json`. Run it once with network, commit the fixtures (they are public data; strip any `mailto`).
- [ ] `SourceAdapter` trait: `fn name(&self) -> &'static str; async fn search(&self, run_id, query, limit) -> RetrievalResult`.
- [ ] Commit `feat(retrieval): add HTTP client, adapter base and recorded fixtures`.

### Task 13: Seven free adapters

**Files:**
- Create: `crates/re-retrieval/src/sources/{openalex,crossref,arxiv,semantic_scholar,europepmc,doaj,dblp,mock}.rs`
- Create: `crates/re-retrieval/tests/sources.rs`

Steps:

- [ ] For each source, port the URL construction and field mapping from the Python module; arXiv via `quick-xml` with a 4 MiB body cap and no DTD handling.
- [ ] Each test serves the recorded fixture through `wiremock`, runs the adapter, and compares the resulting `EvidenceRecord`s to the output of the Python adapter on the same fixture (generate those expectations with a small script into `tests/fixtures/http/<source>.expected.json`; `retrieved_at` and `evidence_id` suffix excluded).
- [ ] Port `MockScholarGatewayAdapter` to `mock.rs` for `REASONING_ENGINE_OFFLINE=1`.
- [ ] Add `OPENALEX_API_KEY` support (`api_key` query param) and record `X-RateLimit-Credits-Used` into the result metadata.
- [ ] Commit `feat(retrieval): port the seven free academic source adapters`.

### Task 14: Aggregator and Scholar Gateway feature

**Files:**
- Create: `crates/re-retrieval/src/{aggregate,sources/scholar_gateway}.rs`, `crates/re-retrieval/tests/aggregate.rs`

Steps:

- [ ] Port `default_adapters` (env `REASONING_ENGINE_OFFLINE`, `REASONING_ENGINE_SOURCES`), `_dedup_key`, `_dedup` (keep best score, accumulate `also_found_in`), sort by `(-score, source_adapter, dedup_key)`, re-id `ev_%04d`, `_aggregate_error` preference order.
- [ ] Fan out with `tokio::JoinSet`; a panic or error in one adapter becomes an `unavailable` error for that source only.
- [ ] Port `scholar_gateway.py` behind Cargo feature `scholar-gateway` (default off, decision 3).
- [ ] Tests: all-sources-fail error aggregation, partial failure, dedup across DOI and title, ordering parity against the Python aggregator on the recorded fixtures.
- [ ] Commit `feat(retrieval): add concurrent aggregator with dedup and ranking`.
- [ ] Open PR `rust/phase-3-retrieval`.

## Phase 4: `re-verify`, `re-mcp`, `re-cli`, parity harness

### Task 15: Claims, lexical verifier, run packs

**Files:**
- Create: `crates/re-verify/src/{lib,claims,verify,runpack,service}.rs`, `crates/re-verify/tests/{claims,runpack}.rs`

Steps:

- [ ] Port `extract_claims` (uses `re-core::claims`), `_overlap_score`, `verify_claims` thresholds 0.65 / 0.35, statuses and rationales verbatim, `method = "term_overlap_mvp"`.
- [ ] `Verifier` trait with `LexicalOverlap` impl; `REASONING_ENGINE_VERIFIER` env read in `service.rs` (default `lexical`, decision 1).
- [ ] Port `runpack.py`: identical artifact names and order, `sort_keys` JSON with 2-space indent and trailing newline, `sources.bib` format, `methodology.md` text, attestation with `engine_version` from `CARGO_PKG_VERSION`, `verify_run_pack_attestation`.
- [ ] Cross-verification tests: a pack produced by the Python CLI (`REASONING_ENGINE_OFFLINE=1 reasoning-engine research ...`) verifies with the Rust verifier, and vice versa.
- [ ] Port `service.py` (`start_run`, `scholar_search`, `run_research_pipeline`, `_build_report`).
- [ ] Commit `feat(verify): port claim verification, quality gate wiring and attested run packs`.

### Task 16: MCP parameter types and validation

**Files:**
- Create: `crates/re-mcp/Cargo.toml`, `crates/re-mcp/src/{lib,params,limits,error}.rs`, `crates/re-mcp/tests/params.rs`

Steps:

- [ ] `limits.rs`: every `MAX_*` constant from `validation.py` with the same values.
- [ ] `params.rs`: one `Deserialize + JsonSchema` struct per tool, field names identical to the Python signatures; custom deserialisers that accept JSON arrays or JSON-encoded array strings (`_coerce_json_array`) and ints or numeric strings (`_coerce_int`), because Notion sends strings.
- [ ] `error.rs`: validation failures map to `McpError` with code `-32001`-style implementation-defined codes in `-32000..-32019` and the same messages as Python.
- [ ] Tests mirroring `tests/test_server.py` validation cases (score out of range, too many trace steps, non-http source URL, bool-as-int rejection).
- [ ] Commit `feat(mcp): add tool parameter types with Python-compatible coercion`.

### Task 17: The 20 tools over stdio

**Files:**
- Create: `crates/re-mcp/src/tools/*.rs` (one file per tool), `crates/re-mcp/src/server.rs`, `crates/re-mcp/tests/tools.rs`

Steps:

- [ ] Implement each tool as an `AsyncTool` returning `Json<T>` with the same JSON keys as the Python `_json_response` payloads: `init_research_session`, `register_branch`, `score_branch`, `select_next_branches`, `record_reflection_tool`, `check_termination`, `get_session_state`, `consensus_candidates`, `save_to_memory`, `recall_memory_tool`, `sanitize_content`, `plan_research_angles_tool`, `evidence_gap_questions_tool`, `start_research_run`, `classify_research_mode_tool`, `scholar_search_tool`, `get_scholar_auth_status`, `run_research_pipeline_tool`, `run_quality_gate_tool`, `export_run_pack_tool`.
- [ ] Register them in `server.rs` in exactly that order; set `annotations(read_only_hint = true)` on the read-only ones (`get_session_state`, `consensus_candidates`, `recall_memory_tool`, `sanitize_content`, `plan_research_angles_tool`, `evidence_gap_questions_tool`, `classify_research_mode_tool`, `get_scholar_auth_status`, `run_quality_gate_tool`).
- [ ] `ServerHandler` info: name `reasoning-engine`, instructions string copied from `transport.py`.
- [ ] Tests use `rmcp` `transport-worker` (in-process) to call every tool once against a temp DB and snapshot the JSON with `insta` (IDs and timestamps redacted).
- [ ] Commit `feat(mcp): implement all twenty tools with structured output`.

### Task 18: `re-cli` with `setup`, `mcp`, `research`, `scholar`, `verify-runpack`

**Files:**
- Create: `crates/re-cli/Cargo.toml`, `crates/re-cli/src/{main,config,setup,commands/*}.rs`, `crates/re-cli/tests/cli.rs`

Steps:

- [ ] `config.rs`: `~/.reasoning-engine/config.toml` (`data_dir`, `verifier`, `http.{enabled,host,port,path,token_file}`, `ui.enabled`, `tunnel.enabled`), env overrides `REASONING_ENGINE_DB`, `REASONING_ENGINE_RUNS_DIR`, `REASONING_ENGINE_VERIFIER`.
- [ ] `setup.rs` with `inquire`: refuse to prompt when stdin is not a TTY (exit 2 with hint); questions from plan section 3.3 in that order; `--yes` accepts defaults; `--reset` wipes config only; offer `claude mcp add` and run it only on explicit Yes.
- [ ] `mcp` subcommand: never reads a TTY, prints the missing-config hint to stderr once, serves stdio.
- [ ] `research`, `scholar search`, `verify-runpack` subcommands with the same flags and JSON output (`indent=2`, sorted keys) as `cli.py`.
- [ ] `serve` exists only with `--features local-host` (Task 20).
- [ ] Tests: `assert_cmd` for argument validation, setup refusing without TTY, hint printed once, `research` producing a verifying run pack offline.
- [ ] Commit `feat(cli): add reasoning-engine binary with setup wizard and stdio MCP`.

### Task 19: Parity harness and startup benchmark

**Files:**
- Create: `tests/parity/run_parity.py`, `tests/parity/script.json`, `tests/parity/README.md`
- Modify: `.github/workflows/rust.yml`, `scripts/bench-startup.sh`

Steps:

- [ ] `script.json`: the exact tool sequence the skill performs (init, plan angles, 3x register/score, select, reflect, terminate, consensus, save memory, recall, start run, scholar search offline, pipeline, gate, export).
- [ ] `run_parity.py`: with the Python `mcp` client, spawn `.venv/bin/reasoning-engine mcp` and `target/release/reasoning-engine mcp` with `REASONING_ENGINE_OFFLINE=1` and separate temp DBs, run the script against both, capture `structuredContent` (Rust) and parsed text JSON (Python), normalise IDs/timestamps/paths, and `difflib` the results. Exit non-zero on any difference.
- [ ] Add the harness to CI (ubuntu) after a release build.
- [ ] Run `scripts/bench-startup.sh` and paste the `hyperfine` table into the PR body.
- [ ] Commit `test(parity): add Python-vs-Rust MCP parity harness`.
- [ ] Open PR `rust/phase-4-server`. Exit criterion: parity diff empty.

### Task 20: Local host feature: HTTP server and bearer auth

**Files:**
- Create: `crates/re-mcp/src/http.rs`, `crates/re-cli/src/commands/serve.rs`, `crates/re-mcp/tests/http.rs`

Steps:

- [ ] Behind `local-host`: `StreamableHttpService` mounted on `axum` at `config.http.path`; `StreamableHttpServerConfig` with `allowed_hosts = [host]`, `allowed_origins = []` (deny cross-origin), `max_request_body_bytes = 1 MiB`, `legacy_session_mode = true` for older clients.
- [ ] Bearer middleware using `subtle::ConstantTimeEq`; token read from `token_file` or `--bearer-token-env`; never logged (print `<redacted>` like Python).
- [ ] `validate_http_bind` port: public hosts require `--unsafe-bind-public`; port range check.
- [ ] `serve` refuses with exit 2 and the `setup` hint when `http.enabled = false` and `--http` absent.
- [ ] Tests mirroring `tests/test_http_transport.py` (Host/Origin rejection, token redaction, public bind refusal).
- [ ] Commit `feat(http): add opt-in local Streamable HTTP server behind local-host feature`.

## Phase 5: semantic verifier, directional DORA, FTS5 memory

### Task 21: Hybrid verifier (`semantic` feature)

**Files:**
- Create: `crates/re-verify/src/verify/hybrid.rs`, `crates/re-verify/src/models_dir.rs`, `crates/re-verify/tests/hybrid.rs`

Steps:

- [ ] Model directory `~/.reasoning-engine/models` with SHA-256 manifest; download only when `REASONING_ENGINE_OFFLINE != 1`; log one line before downloading.
- [ ] `tantivy` in-memory index over evidence snippets (BM25) plus `fastembed` `BAAI/bge-small-en-v1.5` cosine; fuse with RRF `k = 60`; top 5 per claim; status thresholds on the fused rank and cosine as documented in plan 6.1 for the hybrid tier.
- [ ] Tests run only when the model is present (`#[ignore]` otherwise) plus a deterministic test with a tiny fake embedder behind a trait.
- [ ] Commit `feat(verify): add hybrid BM25 + embedding verifier tier`.

### Task 22: NLI verifier (`semantic` feature)

**Files:**
- Create: `crates/re-verify/src/verify/nli.rs`, `scripts/export-nli-onnx.sh`, `crates/re-verify/tests/nli.rs`

Steps:

- [ ] `scripts/export-nli-onnx.sh`: `optimum-cli export onnx --model cross-encoder/nli-deberta-v3-xsmall` into the models dir; record label order `contradiction, entailment, neutral` in `manifest.json`.
- [ ] `nli.rs`: tokenizer via `tokenizers`, `ort` session, softmax over three logits, mapping from plan 6.1 (entailment >= 0.7 supported; contradiction >= 0.7 contradicted; entailment in [0.4, 0.7) partially supported; else unsupported).
- [ ] `high_stakes` mode selects `nli` when built with `semantic`, else logs a warning and uses the configured tier.
- [ ] Tests on 20 hand-labelled pairs committed under `tests/fixtures/nli/pairs.json` (ignored without the model).
- [ ] Commit `feat(verify): add NLI cross-encoder verifier tier`.

### Task 23: Directional DORA, FTS5 memory, labelled eval set

**Files:**
- Create: `crates/re-core/src/dora/directional.rs`, `crates/re-store/src/memory_fts.rs`, `tests/fixtures/verify/labelled_pairs.json`, `crates/re-cli/src/commands/bench.rs`

Steps:

- [ ] `directional.rs` per plan 6.3 (directions from angle IDs, top-2 mean quality, softmax allocation with temperature tied to difficulty, UCB1 with progressive widening `children < ceil(visits^0.5)`); legacy `select_branches` unchanged; `select_next_branches` gains optional `directions` output when `REASONING_ENGINE_DORA=directional`.
- [ ] FTS5 virtual table `episodic_memory_fts` built additively; `recall_memory` uses BM25 when the table exists, legacy overlap otherwise; parity test that both return the same top result on the Phase 2 fixture.
- [ ] Build `labelled_pairs.json` (100 claim/evidence pairs from existing run packs and fixtures, labels `supported`, `contradicted`, `neutral`).
- [ ] `reasoning-engine bench verify` prints precision/recall per tier on that set.
- [ ] Commit `feat(core): add directional DORA, FTS5 memory recall and verifier benchmark`.
- [ ] Open PR `rust/phase-5-semantic`. Exit criterion: NLI contradiction recall > lexical on the labelled set (lexical is 0 by construction).

## Phase 6: UI, packaging, cut-over

### Task 24: Audit UI (`local-host`), Docker, docs, deprecation

**Files:**
- Create: `crates/re-ui/Cargo.toml`, `crates/re-ui/src/lib.rs`, `crates/re-ui/templates/*.html`
- Modify: `Dockerfile`, `README.md`, `docs/architecture.md`, `docs/api-reference.md`, `docs/examples.md`, `.github/workflows/ci.yml`, `pyproject.toml`
- Create: `.github/workflows/release.yml`

Steps:

- [ ] `re-ui`: `askama` templates for session list, branch tree (inline SVG), scores over rounds, evidence table with risk flags; mounted at `/ui` behind the same bearer token; read-only.
- [ ] Dockerfile: multi-stage, `cargo build --release --features local-host`, final `gcr.io/distroless/static` with CA certs, `CMD ["reasoning-engine", "serve", "--http", "--host", "0.0.0.0", "--unsafe-bind-public", "--bearer-token-env", "REASONING_ENGINE_HTTP_TOKEN"]`.
- [ ] `release.yml`: tag-triggered, builds static binaries for linux-x86_64 (musl), linux-aarch64, macos-universal, windows-x86_64; attaches SHA-256 sums.
- [ ] README: installation via release binary or `cargo install --path crates/re-cli`, `setup` walkthrough, `.mcp.json` example pointing at the binary, feature matrix table, deprecation notice for the Python package with a removal version.
- [ ] `pyproject.toml`: bump to `0.2.0`, add a startup `DeprecationWarning` in `cli.py` only (the single allowed Python edit).
- [ ] Commit `feat(ui): add opt-in audit UI`, `chore(release): add static binary and distroless image release`, `docs: cut over to the Rust engine and deprecate the Python package`.
- [ ] Open PR `rust/phase-6-cutover`. Exit criterion: a user swaps the `command` line in `.mcp.json` and `/deep-research` runs end to end.

## Self-Review

- Every Python module has a destination task (plan 3.1 mapping): difficulty 3, dora 4, ucb 5, sanitizer 6, research/profiles/claims/quality 7, db/sessions/memory 9-10, store/models 11, sources 12-13, aggregator 14, claims/runpack/service 15, validation 16, server 17, cli/transport 18 and 20.
- Every parity hazard in plan section 11 is named in the task that meets it.
- Decisions 1-3 are consumed in Tasks 15, 18, 14 respectively; 4 in Task 18; 5 in the Phase 7-12 plan.
- Local host is created only in Task 20 and is feature-gated; nothing before it opens a port.
