# Changelog

## Raouf: 2026-11-27 (Australia/Sydney): faster search, sentence-level NLI, verification against the run's own evidence

**Scope:** `re-retrieval` (aggregate), `re-verify` (nli, models, claims, service), `re-mcp` (schema, tools), `re-cli` (features, search wiring), `skill/deep-research.md`, `README.md`, two scripts.

**Why:** a research session on 26 November ended with the gate blocking 140 of 149 claims. Three causes, all in the engine: the pipeline verified the draft against one fresh ten-record search on the query as typed, not against the papers the session had found; searches within a run overwrote each other's evidence; and every Markdown heading and table row was counted as a claim. Searches also took about 25 seconds each.

**Summary**
- **Search no longer waits for the slowest source.** `FanoutPolicy` (default: three sources with evidence, then 1.5 s of grace, 8 s at most once anything is in hand) is applied to live searches. A source that is cut off is reported as `deadline_exceeded` in the diagnostics and left to finish in the background, so a cached source still stores its answer. `REASONING_ENGINE_FANOUT_DEADLINE_SECS=0` restores the old behaviour. Recorded runs and replays build their aggregator without a policy and wait for every source, so replays stay byte-identical. Measured on 2026-11-27: Europe PMC took 8.5 to 16 s alone and a full search 21.7 to 25.0 s; with the policy, single searches returned in 2.7 to 4.3 s, and five searches driven over MCP took 11.4 s against 69.4 s for the previous build.
- **One paper, one place.** After the DOI/title dedup, records whose titles are the same letters and digits (24 or more) and whose years are within two are merged: the best-scoring record is kept, takes an abstract or DOI from the others if it lacks one, and lists them in `also_found_in`. A paper with a publisher DOI, an arXiv DOI and a DOI-less arXiv record used to take three of ten places.
- **A run's evidence accumulates.** Every search numbers its results from `ev_0001` and the store keys evidence by (run, ID), so a second `scholar_search_tool` call in a run replaced the first one's records. Later searches now continue the numbering, and a work the run already holds keeps its ID. A run's first search is unchanged.
- **`nli` reads sentences, not only abstracts.** Each shortlisted snippet is judged whole and in up to six one- and two-sentence windows (the ones sharing the most terms with the claim); the snippet's entailment is the highest of them. Contradiction is still taken from the whole snippet, capped at one minus the entailment. **This changes verdicts**, so a replay of an older pack verified with `nli` will differ. On SciFact dev (340 pairs): accuracy 0.479 to 0.585, macro F1 0.411 to 0.554; supported precision 1.000 to 0.844 and recall 0.087 to 0.391; contradicted precision 0.536 to 0.569 and recall 0.423 to 0.408; neutral recall 0.924 to 0.885. The cost is time: 180 s for the 340 pairs (530 ms a pair) against about 75 s before, on 8 CPUs. On the 147 labelled pairs nothing moved (0.912). Two alternatives were measured and rejected: contradiction from any single sentence (accuracy 0.541, contradiction precision 0.423) and from single sentences at p >= 0.9 (0.574, 0.494), against 0.585 and 0.569 for the rule kept.
- **NLI batches by length.** A batch is padded to its longest pair, so pairs are sorted by length and a batch ends where the next pair is more than twice as long as its shortest. Scores are returned in input order.
- **Markdown drafts are read as prose.** When a draft has a heading, table row, code fence or list item, headings, tables, rules and fenced code are dropped, everything under a Sources/References heading is dropped, list items become sentences, and an unterminated paragraph gets a full stop. Plain-prose drafts extract exactly as before, so the parity script is unaffected; for Markdown drafts the Rust engine now differs from the deprecated Python one on purpose.
- **`reuse` feature: `run_research_pipeline_tool(evidence_run_id)`.** The draft is verified against the evidence that run gathered (copied into the new run and recorded as an `evidence_reused` provenance event) and no new search is made; if that run holds none, the pipeline searches as usual. In this mode the result carries `evidence_count`, `verifier` and a `summary` (claims by status, and up to 25 claims needing attention with the verifier's reason) in place of the evidence records and `verified_against_snippets`. Without the argument the tool answers exactly as before.
- **Skill.** `skill/deep-research.md`: short keyword queries, evidence IDs that persist across a run, `evidence_run_id` in the verify phase, first-hand measurements kept out of the draft, what each verifier's verdict is worth, a floor of three angles for explicitly thorough requests, and the slop audit handed to the stop-slop skill's own workflow.
- **Scripts.** `scripts/build-local.sh` builds with the model tiers on macOS, where the ONNX runtime needs the compiler runtime linked (`___isPlatformVersionAtLeast`). `scripts/e2e_mcp_verify.py` drives the MCP server over stdio through search, accumulation and verification, with timings.

**Files changed**
- `crates/re-retrieval/src/aggregate.rs`, `crates/re-retrieval/tests/aggregate.rs`
- `crates/re-verify/src/{nli,models,claims,service}.rs`, `crates/re-verify/tests/service.rs`
- `crates/re-mcp/{Cargo.toml,src/schema.rs,src/tools.rs,tests/params.rs,tests/tools.rs}`
- `crates/re-cli/{Cargo.toml,src/main.rs}`
- `skill/deep-research.md`, `README.md`, `AGENT.md`, `CHANGELOG.md`, `scripts/build-local.sh`, `scripts/e2e_mcp_verify.py`

**Verification**
- `cargo test --workspace --locked` (default features, `REASONING_ENGINE_OFFLINE=1`, macOS on Apple silicon): 253 passed, 0 failed (236 before; 17 new).
- `cargo test --workspace --locked --features local-host,trust,tasks,fulltext,graph,learn,fa,reuse,semantic`: 334 passed, 0 failed.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` on the default features, on `local-host,trust,tasks`, and on the nine-feature set above: clean. `cargo fmt --all --check`: clean. `cargo doc --workspace --no-deps`: no warnings.
- `reasoning-engine bench verify --tier lexical --tier hybrid --tier nli`: 0.585 / 0.571 / 0.912 on the 147 labelled pairs, unchanged. `bench verify --dataset scifact --tier nli`: the figures above.
- `scripts/e2e_mcp_verify.py` (live network, throwaway database, one nine-claim Markdown draft with two planted false claims). New build with `nli` and `evidence_run_id`: 40 evidence records kept across five searches, 9 claims extracted, 6 supported, 1 partially supported, the two false claims marked `contradicted` and `unsupported`, verification 36.5 s, result 1.6 KB. Previous build with `lexical`: 8 records left in the run after five searches, 16 "claims" (headings and table rows included), 0 supported, 10 unsupported, 6 partially supported, verification 5.1 s, result 31.5 KB.
- Python parity (`tests/parity/run_parity.py` with the default-feature build and the Python package in a fresh venv): 46 steps, identical after normalisation.
- Not run locally: `--all-features` (`wasm-adapters`, `scholar-gateway`), the MSRV job and `cargo deny`. No dependency changed, so `Cargo.lock` is untouched.

**Follow-ups**
- Verification with `nli` is slower than `lexical` was, because it now does inference: about 4 s a claim with five shortlisted snippets, plus the `graph` contradiction searches. An early exit when the top snippet already entails the claim, or a shortlist of three, would cut it; both change verdicts and need a multi-evidence benchmark first, which `bench verify` (one snippet per pair) is not.
- The NLI model is still `nli-deberta-v3-xsmall`. A grounding-specific checker (the MiniCheck family, Tang et al., EMNLP 2024) is the obvious next comparison on SciFact and on the labelled pairs.
- Europe PMC rarely answers inside the deadline, so biomedical queries now mostly miss it on the first ask. A longer deadline for `high_stakes` medical runs, or a per-source timeout, would fix that; `REASONING_ENGINE_FANOUT_DEADLINE_SECS=0` is the workaround.
- DBLP answers with an HTML bot-check page (HTTP 202), reported as `malformed_response`. It should be dropped from the default sources or reported as `unavailable`.
- OpenAlex has required a key since February 2026 and Semantic Scholar rate-limits keyless calls. Set `OPENALEX_API_KEY` and `S2_API_KEY` in the MCP server's environment; `setup` could ask for them.
- The pipeline summary and compact result exist only with `evidence_run_id`. Offering them for the plain call needs the Python side changed or the parity script taught to ignore them.
- Tag v0.2.1 (unchanged from the entries below). The installed binary is again a local build of this branch.

## Raouf: 2026-11-17b (Australia/Sydney): `cargo deny` advisories: rmcp 3.9.0, rusqlite 0.41.0

**Scope:** `Cargo.toml`, `Cargo.lock`, `deny.toml`. No source change.

**Summary**
- The `deny` job, green on 22 October, failed on 17 November on five advisories published in between. None came from a change in this repository.
- `rmcp` 3.5.1 to 3.9.0: RUSTSEC-2026-0457 (OAuth client SSRF filter), RUSTSEC-2026-0461 (unbounded tool schema cache in the Streamable HTTP server transport) and RUSTSEC-2026-0462 (unbounded response buffering in the Streamable HTTP clients, fixed in 3.9.0).
- `rusqlite` 0.40.2 to 0.41.0, which moves `libsqlite3-sys` to 0.39.1: RUSTSEC-2026-0459 (the bundled SQLite, CVE-2026-119375).
- RUSTSEC-2026-0543 flags `chrono` as unmaintained and offers no upgrade. `chrono` is not a direct dependency (it comes through `rmcp` and `schemars`), so it is ignored in `deny.toml` with that reason, as the other unmaintained notices are.
- Both bumps are the smallest patched versions. Nothing in the workspace needed changing to compile against them.

**Files changed**
- `Cargo.toml` (two exact pins), `Cargo.lock`, `deny.toml`, `CHANGELOG.md`.

**Verification**
- `cargo test --workspace --locked --features local-host,trust,tasks` (offline, macOS on Apple silicon): 262 passed, 0 failed.
- `cargo clippy --workspace --all-targets --locked --features local-host,trust,tasks -- -D warnings`: clean. `cargo fmt --all --check`: clean.
- `cargo deny` is not installed locally; the `deny`, `--all-features`, parity and MSRV jobs are checked by CI on the pull request.

**Follow-ups**
- The released v0.2.0 binaries were built with the old versions. Tag v0.2.1 to ship the patched ones.
- `docs/rust-rewrite-plan-2026.md` still names the versions the port started from (rmcp 3.5.1, rusqlite 0.40.2); it is a dated plan and is left as written.

## Raouf: 2026-11-17 (Australia/Sydney): contributing guide for the Rust engine; stale and local details corrected

**Scope:** documentation only, no code change.

**Summary**
- `CONTRIBUTING.md` described the deprecated Python package (virtual environment, `pytest`, `ruff`). It now describes the Rust workflow: the four checks CI runs, why plain `cargo test` must pass as well as nextest, the parity, MSRV and `cargo deny` jobs, and the rules for verdict changes and tool contracts.
- `SECURITY.md` listed 0.1.x as the supported version. It now lists 0.2.x (the Rust engine) and marks the 0.1.x Python package as deprecated.
- `MCP_AUDIT_2026-06-21.md` named a path on the maintainer's machine; it now says "this repository". A note at the top says the audit covers the Python server of June 2026.
- `AGENT.md` opens with a note for human readers and no longer says the contributing guide is out of date.
- Repository topics added on GitHub (`mcp`, `rust`, `claude-code`, `research`, `verification`).
- README checked against the code, no change needed: 20 MCP tools, seven search adapters, 147 labelled pairs in `tests/fixtures/verify/labelled_pairs.json`, minimum Rust 1.89, release v0.2.0.

**Files changed**
- `CONTRIBUTING.md`, `SECURITY.md`, `MCP_AUDIT_2026-06-21.md`, `AGENT.md`, `CHANGELOG.md`.

**Verification**
- `cargo test --workspace --locked` (default features, `REASONING_ENGINE_OFFLINE=1`, macOS on Apple silicon): 236 passed, 0 failed.
- `gitleaks detect` over all 113 commits: no leaks found.
- Not run locally: the `--all-features` suite, the parity job and `cargo deny`. CI runs them on the pull request.

**Follow-ups**
- Tag v0.2.1 (unchanged from the entry below).

## Raouf: 2026-10-22b (Australia/Sydney): merged PR #20; installs moved to the Rust engine

**Scope:** release/install state, no code change.

**Summary**
- PR #20 (pin race, invented figures, deep-research skill) merged into `master` as `080a5f1` after all 8 CI checks passed (test on macOS, Ubuntu and Windows; lint; deny; msrv; parity; Python test).
- The maintainer's Claude Code and Codex now run the Rust engine instead of the Python package: `~/.local/bin/reasoning-engine mcp`, registered user-wide in both (`claude mcp add -s user`, `codex mcp add`). The Python `.venv` and build artefacts were removed from the checkout; the Python source stays until 0.3.0.
- Data moved with it: the Python database was copied with SQLite's online backup to `~/.reasoning-engine/reasoning.db` (integrity check ok; 153 sessions, 130 research runs, 21 memories) and the 53 run packs to `~/.reasoning-engine/runs`. The original is kept as `~/.reasoning-engine/reasoning.python-backup.db`.
- The installed binary is a local release build of `master` with the release features (`local-host,trust,tasks`), because v0.2.0 predates PR #20. The signed v0.2.0 binary is kept as `~/.local/bin/reasoning-engine.v0.2.0-release`.
- The `deep-research` skill from `master` is installed as `SKILL.md` for both clients.

**Files changed**
- `CHANGELOG.md`, `AGENT.md`.

**Verification**
- `claude mcp get reasoning-engine`: connected; `codex mcp list`: enabled; both point at `~/.local/bin/reasoning-engine`.
- A recall over MCP returned memories from the migrated database.
- Installed skill files are byte-identical to `skill/deep-research.md` on `master`.

**Follow-ups**
- Tag v0.2.1 so the installed binary can return to a signed, attested release.

## Raouf: 2026-10-22 (Australia/Sydney): pin race, invented figures, deep-research skill

**Scope:** `re-model` (pin, ids, clock), `re-verify` (replay, numeric), `skill/deep-research.md`, `README.md`.

**Summary**
- Pins no longer overlap. A second `pin()` waits until the first guard drops, instead of resetting the stream and then unpinning the other run mid-pipeline. Plain `cargo test` was failing the two byte-for-byte replay tests every time and the pin unit test about one run in three; CI did not see it because nextest runs each test in its own process.
- `RecordPlan::now` takes its clock and seed from the operating system (`clock::system_utc_now`, `ids::os_random_u64`), never from a pin another run holds. Before, planning a recording during someone else's pinned run consumed that run's ID stream, inherited its fixed clock and got a predictable seed.
- `numeric::enforce` (feature `trust`) now drops a quantitative claim one step when none of its numbers is in the evidence it cites: `supported` to `partially_supported` as before, and now `partially_supported` to `unsupported`, which cites nothing and blocks the gate. A draft saying "RAG eliminates 100% of hallucinations" over evidence that only says RAG reduces them used to pass with a warning.
- `deep-research` skill rewritten for the Rust engine and for Codex as well as Claude Code: bare tool names with a per-client note, argument rules the server enforces (scores 0-1, source objects), `prefetch` with each angle's `search_query`, the real gate results (`blocked`, `pass_with_warnings`, `pass`) and what to do for each, `verify-runpack` and record/replay, and stop-slop/docx used only when installed.
- README: install the skill as `skills/deep-research/SKILL.md` for both clients (the old `cp` put it where neither loads it), and register with Codex.

**Files changed**
- `crates/re-model/src/pin.rs`: `Condvar`-serialised pins; test-only `serial()` lock; regression test `concurrent_pins_take_turns_instead_of_clobbering_each_other`.
- `crates/re-model/src/ids.rs`, `clock.rs`: `os_random_u64`, `system_utc_now`; tests take `serial()`.
- `crates/re-model/src/records.rs`: run-creation test takes `serial()`.
- `crates/re-verify/src/replay.rs`: `new_seed` and `RecordPlan::now` ignore pins.
- `crates/re-verify/tests/replay.rs`: `a_plan_made_during_another_pinned_run_leaves_its_pin_alone`.
- `crates/re-verify/src/numeric.rs`: one-step downgrade, clears citations on `unsupported`.
- `crates/re-verify/tests/trust_pipeline.rs`: `an_invented_number_on_weak_evidence_is_unsupported_and_blocks`.
- `skill/deep-research.md`, `README.md`.

**Verification**
- `cargo test --workspace --locked --no-fail-fast` (parallel, default features): 236 passed, 0 failed, five runs in a row. Before: 2 replay tests failed every run.
- `cargo test -p re-model --lib` 25/25 and `-p re-verify --test replay` 25/25 in parallel (before: 10/15 and 0/8).
- `cargo test --workspace --locked --all-features --no-fail-fast` (parallel): 325 passed, 0 failed, 7 ignored.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: clean. `cargo fmt --all --check`: clean.
- Release build (`local-host,trust,tasks`): 24/24 MCP tool calls pass end to end; the "100% / NASA 1987" draft is `unsupported` and the gate is `blocked` (v0.2.0: `partially_supported`, `pass_with_warnings`); a true claim still passes.

**Follow-ups**
- Replaying a v0.2.0 pack whose quantitative claim was `partially_supported` with a missing number now reports a difference in `claim_verifications.json`: the verdict changed by design.
- Tag a release so the installed binaries pick these fixes up.
