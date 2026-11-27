# Agent notes

> **For human readers.** This file is addressed to AI coding assistants that work on this repository: the rules they follow and a short log of what they changed. `docs/superpowers/` holds the specs and plans they worked from. Start with [README.md](README.md) and [CONTRIBUTING.md](CONTRIBUTING.md).

Working rules for agents changing this repo. The Rust workspace under `crates/` is the engine; the Python package under `src/` is deprecated.

## Checks before a PR

Match CI (`.github/workflows/rust.yml`):

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo nextest run --workspace --all-features   # or: cargo test --workspace --all-features
cargo doc --workspace --no-deps
```

Plain `cargo test` must pass too, not only nextest: nextest runs each test in its own process and hides races on process-wide state.

## Process-wide state

- `re_model::pin` pins the clock and ID stream for the whole process. Pins are serialised: never nest one on a thread that holds one (it deadlocks). Unit tests in `re-model` that read the clock or draw IDs take `crate::pin::serial()`.
- Anything that must stay random or real-time while another run may be pinned (seeds, a new recording's clock) uses `ids::os_random_u64` / `clock::system_utc_now`, not `short_hex` / `utc_now`.

## Verification semantics

- Gate results are `pass`, `pass_with_warnings` and `blocked`. `unsupported`, `contradicted` and `needs_more_evidence` block; `partially_supported` warns.
- With `trust`, a quantitative claim whose numbers are absent from its cited evidence drops one step (`supported` to `partially_supported`, `partially_supported` to `unsupported`). Changing a verdict rule changes replays of older packs: note it in `CHANGELOG.md`.

## Search, evidence and the pipeline

- Live searches run under `FanoutPolicy` and do not wait for a slow source. Anything recorded or replayed must build its aggregator without one (`AcademicSearchAggregator::new`, no `with_fanout`): cutting a source off depends on the clock.
- A run's evidence accumulates across `scholar_search` calls; IDs continue and a work keeps its ID. Do not renumber from `ev_0001` in anything that saves evidence into an existing run.
- Behaviour the Python package does not have goes behind a Cargo feature or leaves plain-input behaviour untouched. The parity job compares the default build with Python, and `crates/re-core/tests/golden.rs` pins the pure functions. `reuse` (the `evidence_run_id` argument) follows the pattern of `tasks` and `learn`: gate the schema parameter and strip it in `crates/re-mcp/tests/params.rs`.
- The `nli` tier pools sentence windows for entailment and reads contradiction from the whole snippet. Any change to that pooling is a verdict change: measure it with `bench verify` on both sets and log the numbers.
- On macOS, build `semantic` with `scripts/build-local.sh`; a plain `cargo build` fails at link time.

## The skill

`skill/deep-research.md` is the source for both clients; install it as `~/.claude/skills/deep-research/SKILL.md` and `~/.codex/skills/deep-research/SKILL.md`. Refer to tools by bare MCP name. Keep its argument rules and gate results in step with `crates/re-mcp` and `crates/re-core/src/gate.rs`.

## Log

### Raouf: 2026-10-22 (Australia/Sydney)
- Serialised pins and made `RecordPlan::now` pin-independent (plain `cargo test` was failing replay tests).
- Invented figures on weak evidence are now `unsupported` under `trust`.
- Rewrote the `deep-research` skill for the Rust engine, Claude Code and Codex.
- Verification and follow-ups: see `CHANGELOG.md`.

### Raouf: 2026-10-22b (Australia/Sydney)
- PR #20 merged (`080a5f1`). Claude Code and Codex now run the Rust binary at `~/.local/bin/reasoning-engine` (a local build of `master`, pending a v0.2.1 tag); data lives in `~/.reasoning-engine/`. See `CHANGELOG.md`.

### Raouf: 2026-11-17 (Australia/Sydney)
- `CONTRIBUTING.md` now describes the Rust workflow; keep it in step with `.github/workflows/rust.yml` when a CI check changes.
- Do not write machine paths (`/Users/...`) into tracked files; say "this repository" or use `~`.
- Verification and follow-ups: see `CHANGELOG.md`.
- `rmcp` is pinned at 3.9.0 and `rusqlite` at 0.41.0 for the advisories listed in `CHANGELOG.md` (2026-11-17b). Do not pin below them. A new `cargo deny` failure is either fixed by an upgrade or ignored in `deny.toml` with a written reason, never left red.

### Raouf: 2026-11-27 (Australia/Sydney)
- Search fan-out deadline, same-work merge, evidence accumulation within a run, sentence-window NLI with length-aware batching, Markdown-aware claim extraction, and the `reuse` feature (`run_research_pipeline_tool(evidence_run_id)` with a verdict summary).
- `skill/deep-research.md` updated to match; installed copies must stay byte-identical to it.
- Numbers, verification and follow-ups: see `CHANGELOG.md`.
