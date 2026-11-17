# Contributing to Reasoning Engine

The engine is the Rust workspace under `crates/`. The Python package under `src/` is deprecated: it stays for 0.2.x so that the parity job can compare the two servers, and it is removed in 0.3.0. New work goes into the Rust code.

## Development setup

1. Install Rust with rustup. `rust-toolchain.toml` pins the toolchain, and rustup installs it on the first build. The minimum supported version is 1.89 (1.92 for the `semantic` feature, 1.97 for `wasm-adapters`).
2. Clone the repository and build:

   ```bash
   cargo build --workspace
   ```

3. Optional, for the parity check only: Python 3.12 and `pip install -e .` in a virtual environment.

## Checks before a pull request

These match `.github/workflows/rust.yml`:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo nextest run --workspace --all-features    # or: cargo test --workspace --all-features
cargo doc --workspace --no-deps
```

Plain `cargo test` must pass as well as nextest. nextest runs each test in its own process, which hides races on process-wide state; `AGENT.md` explains the one this project has (the pinned clock and ID stream).

CI also runs:

- **parity**: `tests/parity/run_parity.py` starts the Python and the Rust MCP servers offline, runs the same tool script against both and fails on any difference (see `tests/parity/README.md`);
- **msrv**: `cargo +1.89 check --workspace --all-targets`;
- **deny**: `cargo deny` with `deny.toml` (licences and advisories);
- the deprecated Python package's own tests and `ruff` (`.github/workflows/ci.yml`), until 0.3.0.

Tests do not need network access or an API key. Retrieval tests replay recorded HTTP from `tests/fixtures/http/`, `REASONING_ENGINE_OFFLINE=1` makes the engine use deterministic mock evidence, and the tests that download models are marked `#[ignore]`.

## Code standards

- No warnings: CI builds with `RUSTFLAGS=-D warnings` and `RUSTDOCFLAGS=-D warnings`.
- Write the failing test first, then the change.
- A change to a verdict rule changes how older run packs replay. Say so in `CHANGELOG.md`.
- Tool names, arguments, results and error messages are part of the contract that the parity job checks. Change them in both servers or not at all while the Python package exists.
- Keep `skill/deep-research.md` in step with `crates/re-mcp` and `crates/re-core/src/gate.rs`.

## Pull requests

1. Create a branch and make the change with its tests.
2. Run the checks above.
3. Add an entry to `CHANGELOG.md` in the existing format (scope, summary, files changed, verification, follow-ups).
4. Open the pull request with a description of what changed and how it was verified.

## Commit convention

Conventional commits: `feat:`, `fix:`, `refactor:`, `docs:`, `test:`, `chore:`.
