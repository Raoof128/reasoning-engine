# Changelog

## Raouf: 2026-11-27 (Australia/Sydney), second pass: whole abstracts for the verifier, an optional larger NLI model, labelled merges

**Scope:** `re-retrieval` (base, aggregate), `re-verify` (verify, models, replay, error), `re-cli` (main), `README.md`, `AGENT.md`, tests.

**Why:** a second look for accuracy, measured before changing anything. The first hypothesis was wrong and is recorded here so it is not tried again.

**Summary**
- **Abstracts are no longer cut at 1,500 characters in builds with `semantic`.** Every source's abstract went through `clip_snippet(…, 1500)`. 127 of the 340 SciFact dev abstracts are longer (31 past 2,500, 5 past 4,000), and an abstract states its conclusion at the end, so the sentence a claim rests on was often the part cut off. The cap is now a process setting (`re_retrieval::base::snippet_limit`): 1,500 in the library and in the default build, which keeps the Python parity, 4,000 when the CLI is built with `semantic`, and `REASONING_ENGINE_SNIPPET_CHARS` (200 to 20,000) to set it by hand. SciFact dev, 340 pairs, 1,500 to 4,000 characters: `nli` 0.585 to 0.626 (macro F1 0.554 to 0.597; supported precision 0.844 to 0.857 and recall 0.391 to 0.478; contradicted precision 0.569 to 0.644, recall unchanged at 0.408), `lexical` 0.538 to 0.565, `hybrid` 0.582 to 0.588. The labelled pairs are single sentences and did not move (0.585 / 0.571 / 0.912). **This changes verdicts.**
- **Run packs record the cap.** `replay.json` carries `snippet_chars` when it is not 1,500, and a replay sets the cap to the recorded value (1,500 when absent) before rebuilding evidence, so packs recorded before today replay as they did.
- **Optional larger NLI model.** `REASONING_ENGINE_NLI_MODEL=base` makes the `nli` tier judge with `cross-encoder/nli-deberta-v3-base` (184M parameters, Apache-2.0, pinned revision and SHA-256, a 739 MB download on first use) in place of the xsmall model (71M). It has the same labels and tokenizer. Measured in the engine: SciFact dev 0.674 against 0.626 (macro F1 0.649 against 0.597; supported precision 0.895 and recall 0.558; contradicted precision 0.727 and recall 0.451), labelled pairs 0.912, the same as xsmall. It costs time: 1,413 ms a pair against 531 ms for xsmall in the same session. The default stays xsmall. A run pack records `nli_model` when it is not `xsmall`, and a replay loads the recorded model. An unknown name is an error at start, not a silent fallback.
- **Merged records say where a borrowed field came from.** The title-and-year merge added earlier today could fill the kept record's abstract, DOI or URL from the record merged into it with nothing to show for it. The kept record's metadata now names the source (`snippet_from`, `doi_from`, `url_from`), carried through later merges, and two records are merged only if their author lists share a name when both have one. Raised by the commit security review.
- **Measured and not the cause: sentence splitting.** The engine's splitter breaks after every full stop, `e.g.` and `S. aureus` included. On SciFact that costs nothing: 0.626 with SciFact's own sentence boundaries, 0.629 with the engine's, 0.626 with an abbreviation-aware splitter. The two-sentence windows absorb a bad split.
- **Measured and left alone: Europe PMC and DBLP.** `resultType=lite` on Europe PMC is no faster (22.5 s and 1.4 s on consecutive identical calls, against 10.6 s and 23.1 s for `core`) and drops the abstract, so the latency is theirs and the fan-out deadline stays the answer. DBLP's search API and its Trier mirror both answer with an Anubis bot check (HTTP 202, HTML); `sparql.dblp.org` answers without one (a title `FILTER` took 6.5 s) and is the route for a future adapter.

**Model comparison** (Python, `transformers` on Apple MPS, the engine's verdict rule reimplemented, whole abstracts; the xsmall row reproduces the engine's figures, 0.626 and 0.912):

| Model | Parameters | SciFact accuracy | Macro F1 | Supported P / R | Contradicted P / R | Labelled pairs |
|---|---:|---:|---:|---:|---:|---:|
| `cross-encoder/nli-deberta-v3-xsmall` (shipped) | 71M | 0.626 | 0.596 | 0.85 / 0.48 | 0.64 / 0.41 | 0.912 |
| `cross-encoder/nli-deberta-v3-small` | 142M | 0.588 | 0.553 | 0.83 / 0.49 | 0.59 / 0.32 | 0.912 |
| `tasksource/deberta-small-long-nli` | 142M | 0.688 | 0.621 | 0.82 / 0.68 | 0.86 / 0.25 | 0.898 |
| `cross-encoder/nli-deberta-v3-base` (now optional) | 184M | 0.674 | 0.649 | 0.90 / 0.56 | 0.73 / 0.45 | 0.912 |
| `MoritzLaurer/DeBERTa-v3-base-mnli-fever-anli` | 184M | 0.703 | 0.675 | 0.79 / 0.67 | 0.72 / 0.47 | 0.884 |

The base cross-encoder was chosen because it is the only candidate that is better on every SciFact measure and no worse on the labelled pairs; the MoritzLaurer model scores higher on SciFact but says `supported` wrongly more often (precision 0.79) and loses three points on the labelled pairs. MiniCheck (Tang et al., EMNLP 2024) was not run: its DeBERTa is 435M parameters with no ONNX export, and it has no contradiction class.

**Files changed**
- `crates/re-retrieval/src/{base,aggregate}.rs`, `crates/re-retrieval/tests/{aggregate,snippet_limit}.rs`
- `crates/re-verify/src/{verify,models,replay,error}.rs`, `crates/re-verify/tests/replay_settings.rs`
- `crates/re-cli/src/main.rs`, `README.md`, `AGENT.md`, `CHANGELOG.md`

**Verification**
- `cargo test --workspace --locked` (default features, `REASONING_ENGINE_OFFLINE=1`): 258 passed, 0 failed. With `--features local-host,trust,tasks,fulltext,graph,learn,fa,reuse,semantic`: 339 passed, 0 failed. Five tests are new: two for merges, two for the snippet cap, one that records and replays packs under each setting.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` on the default features, `local-host,trust,tasks` and the nine-feature set: clean. `cargo fmt --all --check`: clean. `cargo doc --workspace --no-deps` with `-D warnings`: clean.
- Python parity (`tests/parity/run_parity.py`, default build): 46 steps, identical after normalisation.
- `reasoning-engine bench verify --dataset scifact --tier lexical --tier hybrid --tier nli` and the same with `REASONING_ENGINE_SNIPPET_CHARS=1500` and with `REASONING_ENGINE_NLI_MODEL=base`: the figures above. `bench verify` on the labelled pairs: 0.585 / 0.571 / 0.912, and 0.912 with `base`.
- `scripts/e2e_mcp_verify.py` (live, five searches then nine claims against 40 evidence records): searches 13.0 to 14.5 s, verification 36.6 s with the 4,000-character cap and 31.8 s with 1,500; six claims supported, one partially supported and both planted false claims blocking in each. With 1,500 the planted "always more accurate on every benchmark" claim came back `contradicted`; with 4,000 it came back `unsupported`. See the first follow-up.
- `verify-runpack` on a pack recorded on 2026-11-26: valid. `REASONING_ENGINE_NLI_MODEL=huge`: refused with `unknown NLI model`.
- Not run: `--all-features` (`wasm-adapters`, `scholar-gateway`), the MSRV job, `cargo deny`. No dependency changed.

**Follow-ups**
- Contradiction is read from the whole snippet, which a longer snippet dilutes: one live case went from `contradicted` to `unsupported`. Taking the larger of the whole snippet's and its first 1,500 characters' contradiction was measured on SciFact: recall 0.408 to 0.437, precision 0.644 to 0.585, accuracy 0.629 to 0.626. Not adopted, because a wrong `contradicted` misleads more than a missed one, which still blocks the gate as `unsupported`. Worth revisiting with a set that has more contradictions than SciFact's 71.
- The base model's ONNX file is float32. The same repository has an int8 build a third of the size (`model_qint8_arm64.onnx`); it was not measured. If it holds the accuracy it would give most of the gain at something near xsmall's speed.
- OpenAlex (`search.semantic`, a 1,024-dimension Qwen3 embedding index, one request a second, 2,000 characters) and Semantic Scholar (`/snippet/search`, passages of about 500 words from paper bodies) both now offer what the verify phase most needs: retrieval by a whole claim, and text from beyond the abstract. Both answer 429 without a key, so neither could be tested here; the day's keyless OpenAlex budget was spent by these measurements. They are the next adapters to write once `OPENALEX_API_KEY` and `S2_API_KEY` are set.
- Crossref has rate-limited by contact address since 21 July 2026. Every install shares the default one; set `REASONING_ENGINE_CONTACT_EMAIL`.
- A calibration file made with the xsmall model does not fit the base model's scores. Recalibrate after switching.
- Unchanged from the entry below: verification time per claim, the DBLP source, tagging v0.2.1.

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
