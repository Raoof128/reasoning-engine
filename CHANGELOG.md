# Changelog

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
