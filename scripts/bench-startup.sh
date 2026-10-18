#!/usr/bin/env bash
# Cold-start comparison of the Python and Rust CLIs.
# Usage: scripts/bench-startup.sh   (from the repo root)
set -euo pipefail

PY_BIN=".venv/bin/reasoning-engine"
RS_BIN="target/release/reasoning-engine"

if ! command -v hyperfine >/dev/null 2>&1; then
    echo "hyperfine not found; install it with: cargo install hyperfine --locked" >&2
    exit 1
fi
if [[ ! -x "$PY_BIN" ]]; then
    echo "Python CLI missing at $PY_BIN; run: python3 -m venv .venv && .venv/bin/pip install -e '.[dev]'" >&2
    exit 1
fi
if [[ ! -x "$RS_BIN" ]]; then
    # The Rust binary arrives in Phase 4 (Task 18). Until then there is nothing to compare.
    echo "Rust CLI not built yet ($RS_BIN); skipping until Phase 4." >&2
    exit 0
fi

hyperfine --warmup 3 --export-markdown target/bench-startup.md \
    "$PY_BIN --help" \
    "$RS_BIN --help"
echo "Results written to target/bench-startup.md"
