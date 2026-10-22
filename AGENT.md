# Agent notes

Working rules for agents changing this repo. `CONTRIBUTING.md` still describes the deprecated Python package; the Rust workspace under `crates/` is the engine.

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

## The skill

`skill/deep-research.md` is the source for both clients; install it as `~/.claude/skills/deep-research/SKILL.md` and `~/.codex/skills/deep-research/SKILL.md`. Refer to tools by bare MCP name. Keep its argument rules and gate results in step with `crates/re-mcp` and `crates/re-core/src/gate.rs`.

## Log

### Raouf: 2026-10-22 (Australia/Sydney)
- Serialised pins and made `RecordPlan::now` pin-independent (plain `cargo test` was failing replay tests).
- Invented figures on weak evidence are now `unsupported` under `trust`.
- Rewrote the `deep-research` skill for the Rust engine, Claude Code and Codex.
- Verification and follow-ups: see `CHANGELOG.md`.
